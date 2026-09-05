/**
 * AgentContext — high-level action dispatch API.
 *
 * Wraps the raw `ACTION` IPC message into ergonomic, strongly-typed methods
 * that autonomous agents call to interact with the desktop.
 *
 * Obtain an instance via `client.agent()`.
 *
 * ```typescript
 * const ctx = client.agent();
 *
 * // Click a button by node ID
 * await ctx.click(nodeId);
 *
 * // Type into an input field
 * await ctx.type(nodeId, 'hello@example.com');
 *
 * // Scroll a list
 * await ctx.scroll(nodeId, -120); // scroll up
 *
 * // Press a key (Enter = VK 0x0D)
 * await ctx.key(nodeId, 0x0D);
 *
 * // Focus an element
 * await ctx.focus(nodeId);
 * ```
 *
 * All methods wait for the `ACTION_RESULT` acknowledgement from `vactd` before
 * resolving, giving agents a reliable "did it work?" signal.
 */

import type { ITransport } from '../transport/ITransport.js';
import type { ActionResultMessage } from '../types/ipc.js';
import { VactTimeoutError, VactActionError } from '../errors/VactError.js';

// ─────────────────────────────────────────────────────────────────────────────
// AgentContext
// ─────────────────────────────────────────────────────────────────────────────

type PendingAction = {
  resolve: (result: ActionResultMessage) => void;
  reject: (err: Error) => void;
  timer: NodeJS.Timeout;
};

/**
 * High-level agent action dispatcher.
 *
 * Each method sends an `ACTION` message and awaits the corresponding
 * `ACTION_RESULT` acknowledgement — providing end-to-end confirmation that
 * the I/O bus received and executed the action.
 */
export class AgentContext {
  private readonly _transport: ITransport;
  private readonly _pending: Map<string, PendingAction>;
  private readonly _timeoutMs: number;

  /** @internal — instantiate via `client.agent()` */
  public constructor(
    transport: ITransport,
    pending: Map<string, PendingAction>,
    timeoutMs: number,
  ) {
    this._transport = transport;
    this._pending = pending;
    this._timeoutMs = timeoutMs;
  }

  // ── Action methods ────────────────────────────────────────────────────────

  /**
   * Click a scene node.
   *
   * Routes through Direct Mode (`SendInput`) or Ghost Mode (`PostMessageW`)
   * based on the target window type — determined automatically by `vactd`.
   *
   * @param targetId - Stable scene node ID (from `SceneNode.id` or `SceneQuery`)
   * @throws {VactActionError} if the daemon rejects the action
   * @throws {VactTimeoutError} if no acknowledgement arrives within the timeout
   */
  public async click(targetId: number): Promise<ActionResultMessage> {
    return this._dispatch('CLICK', targetId);
  }

  /**
   * Type text into a scene node.
   *
   * Delivers characters via the hardware input queue (Direct Mode) for
   * maximum compatibility with Electron, WebGL, and browser-based UIs.
   *
   * @param targetId - Stable scene node ID
   * @param text - Text to type (Unicode supported)
   */
  public async type(targetId: number, text: string): Promise<ActionResultMessage> {
    return this._dispatch('TYPE', targetId, { text });
  }

  /**
   * Scroll within a scene node.
   *
   * @param targetId - Stable scene node ID
   * @param deltaY - Scroll delta in logical pixels. Negative = scroll up, positive = scroll down.
   */
  public async scroll(targetId: number, deltaY: number): Promise<ActionResultMessage> {
    return this._dispatch('SCROLL', targetId, { delta_y: deltaY });
  }

  /**
   * Press a keyboard key targeting a scene node.
   *
   * @param targetId - Stable scene node ID to focus before the key press
   * @param vk - Windows Virtual Key code (e.g. `0x0D` for Enter, `0x1B` for Escape)
   * @see https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes
   */
  public async key(targetId: number, vk: number): Promise<ActionResultMessage> {
    return this._dispatch('KEY', targetId, { vk });
  }

  /**
   * Move keyboard focus to a scene node without clicking.
   *
   * @param targetId - Stable scene node ID
   */
  public async focus(targetId: number): Promise<ActionResultMessage> {
    return this._dispatch('FOCUS', targetId);
  }

  /**
   * Teach or update a semantic label for a scene node in the persistent memory cache.
   *
   * @param targetId - Stable scene node ID
   * @param label - Human/semantic name (e.g. "Settings", "Mute Audio")
   * @param actionResult - Optional action result tag
   */
  public async learn(
    targetId: number,
    label: string,
    actionResult?: string,
  ): Promise<ActionResultMessage> {
    const extra: { label: string; action_result?: string } = { label };
    if (actionResult !== undefined) {
      extra.action_result = actionResult;
    }
    return this._dispatch('LEARN', targetId, extra);
  }

  // ── Private dispatch ──────────────────────────────────────────────────────

  private async _dispatch(
    action: ActionResultMessage['action'],
    targetId: number,
    extra: { text?: string; delta_y?: number; vk?: number; label?: string; action_result?: string } = {},
  ): Promise<ActionResultMessage> {
    const key = `${action}:${targetId}`;

    await this._transport.send({
      type: 'ACTION',
      action,
      target_id: targetId,
      ...extra,
    });

    return new Promise<ActionResultMessage>((resolve, reject) => {
      const timer = setTimeout(() => {
        this._pending.delete(key);
        reject(new VactTimeoutError(`action:${action}`, this._timeoutMs));
      }, this._timeoutMs);

      this._pending.set(key, {
        resolve: (result) => {
          if (result.ok) {
            resolve(result);
          } else {
            reject(
              new VactActionError(
                action,
                targetId,
                result.error ?? 'Unknown daemon error',
              ),
            );
          }
        },
        reject,
        timer,
      });
    });
  }
}
