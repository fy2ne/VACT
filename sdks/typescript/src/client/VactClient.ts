/**
 * VactClient — the main entry point for the VACT TypeScript SDK.
 *
 * Connects to a running `vactd` daemon over Named Pipe IPC, performs the
 * protocol handshake, and exposes a rich event-driven + async-iterator API
 * for consuming the real-time Vector Scene Graph stream.
 *
 * ## Quick Start
 *
 * ```typescript
 * import { VactClient } from 'vact-sdk';
 *
 * const client = new VactClient();
 * await client.connect();
 *
 * client.on('snapshot', (graph) => {
 *   console.log('Initial scene:', graph.active_window, graph.root.children?.length, 'nodes');
 * });
 *
 * client.on('diff', (diff) => {
 *   console.log(`Frame ${diff.seq}: ${diff.mutations.length} mutations`);
 * });
 *
 * // Or: async iteration (great for LangChain tool loops)
 * for await (const frame of client.frames()) {
 *   if (frame.type === 'SNAPSHOT') { ... }
 *   if (frame.type === 'DIFF') { ... }
 * }
 * ```
 *
 * ## State Access
 *
 * After the first snapshot, call `client.getState()` at any time for the
 * latest hydrated scene graph — no manual diff management needed.
 *
 * ## Action Dispatch
 *
 * ```typescript
 * await client.agent().click(nodeId);
 * await client.agent().type(nodeId, 'hello world');
 * ```
 */

import EventEmitter from 'eventemitter3';

import type { SceneGraph } from '../types/scene.js';
import type { DiffFrame } from '../types/diff.js';
import type {
  ServerMessage,
  SnapshotMessage,
  DiffMessage,
  ActionResultMessage,
} from '../types/ipc.js';
import {
  PROTOCOL_VERSION,
  DEFAULT_CAPABILITIES,
  isHandshakeAck,
  isSnapshot,
  isDiff,
  isActionResult,
} from '../types/ipc.js';
import type { ITransport } from '../transport/ITransport.js';
import { StateManager } from './StateManager.js';
import { resolveOptions } from './VactClientOptions.js';
import type { VactClientOptions } from './VactClientOptions.js';
import { AgentContext } from '../agent/AgentContext.js';
import { SceneQuery } from '../query/SceneQuery.js';
import {
  VactConnectionError,
  VactTimeoutError,
  VactStateError,
} from '../errors/VactError.js';

// ─────────────────────────────────────────────────────────────────────────────
// Client lifecycle state
// ─────────────────────────────────────────────────────────────────────────────

export type ClientState =
  | 'idle'
  | 'connecting'
  | 'handshaking'
  | 'connected'
  | 'disconnected'
  | 'closed';

// ─────────────────────────────────────────────────────────────────────────────
// Frame union for async iteration
// ─────────────────────────────────────────────────────────────────────────────

/** A frame yielded by the {@link VactClient.frames} async iterator. */
export type VactFrame =
  | { type: 'SNAPSHOT'; graph: SceneGraph }
  | { type: 'DIFF'; diff: DiffFrame };

// ─────────────────────────────────────────────────────────────────────────────
// Event map
// ─────────────────────────────────────────────────────────────────────────────

export interface VactClientEvents {
  /**
   * Full scene graph snapshot received.
   * Fired on initial connection and after any state reset.
   */
  snapshot: (graph: SceneGraph) => void;

  /**
   * Temporal delta diff received.
   * Fired on every subsequent frame (including stable frames with 0 mutations).
   */
  diff: (diff: DiffFrame) => void;

  /**
   * Fired when the connection is closed (gracefully or by error).
   * @param reason - Human-readable reason string.
   */
  close: (reason: string) => void;

  /**
   * Fired when an error occurs that does not close the connection
   * (e.g. a sequence mismatch that triggers a re-sync).
   */
  error: (err: Error) => void;

  /** Fired when the client successfully completes the handshake. */
  connected: () => void;

  /** Fired when the client disconnects or closes. */
  disconnected: (reason: string) => void;
}

// ─────────────────────────────────────────────────────────────────────────────
// VactClient
// ─────────────────────────────────────────────────────────────────────────────

/**
 * The VACT SDK client — your single connection to `vactd`.
 *
 * @see {@link https://vact.fy2ne.me/docs/sdk/typescript}
 */
export class VactClient extends EventEmitter<VactClientEvents> {
  private readonly _transport: ITransport;
  private readonly _options: ReturnType<typeof resolveOptions>;
  private readonly _state: StateManager = new StateManager();

  private _clientState: ClientState = 'idle';

  /** Pending action result waiters: action correlation key → { resolve, reject, timer } */
  private _pendingActions = new Map<
    string,
    {
      resolve: (result: ActionResultMessage) => void;
      reject: (err: Error) => void;
      timer: NodeJS.Timeout;
    }
  >();

  /** Resolvers waiting for async frame iteration */
  private _frameQueue: Array<{
    resolve: (value: IteratorResult<VactFrame>) => void;
  }> = [];
  private _frameBuffer: VactFrame[] = [];
  private _framingDone = false;

  public constructor(options: VactClientOptions = {}) {
    super();
    this._options = resolveOptions(options);
    this._transport = this._options.transport;
  }

  // ── Lifecycle ─────────────────────────────────────────────────────────────

  /**
   * Connect to `vactd` and perform the protocol handshake.
   *
   * Resolves when the `HANDSHAKE_ACK` is received and the client is ready
   * to receive scene frames.
   *
   * @throws {VactConnectionError} if the Named Pipe cannot be opened.
   * @throws {VactTimeoutError} if the daemon does not respond to the handshake.
   */
  public async connect(): Promise<void> {
    if (this._clientState !== 'idle' && this._clientState !== 'disconnected') {
      throw new VactConnectionError(
        '',
        `Cannot connect: client is in state "${this._clientState}"`,
      );
    }

    this._clientState = 'connecting';

    // Wire up transport events
    this._transport.on('message', this._onMessage.bind(this));
    this._transport.on('close', this._onClose.bind(this));
    this._transport.on('error', this._onTransportError.bind(this));

    await this._transport.connect();

    this._clientState = 'handshaking';

    // Perform handshake
    await this._doHandshake();

    this._clientState = 'connected';
    this.emit('connected');
  }

  /**
   * Gracefully close the connection to `vactd`.
   *
   * All pending actions will be rejected. The `"close"` event will fire.
   */
  public close(): void {
    if (this._clientState === 'closed') return;
    this._clientState = 'closed';

    // Reject all pending actions
    for (const [, waiter] of this._pendingActions) {
      clearTimeout(waiter.timer);
      waiter.reject(new VactConnectionError('', 'Client closed'));
    }
    this._pendingActions.clear();

    this._transport.close();
    this._state.reset();

    // Signal async iterator completion
    this._framingDone = true;
    for (const waiter of this._frameQueue) {
      waiter.resolve({ value: undefined as unknown as VactFrame, done: true });
    }
    this._frameQueue = [];

    this.emit('close', 'Client closed by application');
    this.emit('disconnected', 'Client closed by application');
  }

  /**
   * Wait until the initial SceneGraph snapshot has been received and hydrated.
   *
   * @param timeoutMs - Maximum time to wait in milliseconds (default 5000)
   * @returns The hydrated SceneGraph
   */
  public async waitForHydration(timeoutMs = 5000): Promise<SceneGraph> {
    if (this._state.isHydrated) {
      return this._state.getState();
    }
    return new Promise<SceneGraph>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.off('snapshot', onSnap);
        reject(
          new VactTimeoutError(
            'initial_snapshot_hydration',
            timeoutMs,
          ),
        );
      }, timeoutMs);

      const onSnap = (graph: SceneGraph) => {
        clearTimeout(timer);
        resolve(graph);
      };

      this.once('snapshot', onSnap);
    });
  }

  // ── State access ──────────────────────────────────────────────────────────

  /**
   * Get the current hydrated scene graph.
   *
   * @throws {VactStateError} if no snapshot has been received yet.
   */
  public getState(): SceneGraph {
    return this._state.getState();
  }

  /** `true` if the client has received at least one scene snapshot. */
  public get isHydrated(): boolean {
    return this._state.isHydrated;
  }

  /** Current lifecycle state of the client. */
  public get clientState(): ClientState {
    return this._clientState;
  }

  // ── Query API ─────────────────────────────────────────────────────────────

  /**
   * Create a {@link SceneQuery} builder for the current scene state.
   *
   * ```typescript
   * const btn = client.query().ofType('BUTTON').withLabel(/send/i).first();
   * ```
   *
   * @throws {VactStateError} if no snapshot has been received yet.
   */
  public query(): SceneQuery {
    const graph = this._state.getState();
    return new SceneQuery(graph.root);
  }

  // ── Agent action API ──────────────────────────────────────────────────────

  /**
   * Get an {@link AgentContext} for dispatching actions to the I/O bus.
   *
   * ```typescript
   * await client.agent().click(nodeId);
   * await client.agent().type(nodeId, 'hello');
   * ```
   */
  public agent(): AgentContext {
    return new AgentContext(this._transport, this._pendingActions, this._options.actionTimeoutMs);
  }

  // ── Async iterator API ────────────────────────────────────────────────────

  /**
   * Async generator that yields every incoming scene frame.
   *
   * Useful for LangChain tool loops, AutoGen step handlers, or any async
   * pipeline that processes frames sequentially.
   *
   * ```typescript
   * for await (const frame of client.frames()) {
   *   if (frame.type === 'SNAPSHOT') {
   *     // Initial scene loaded
   *   }
   *   if (frame.type === 'DIFF' && frame.diff.mutations.length > 0) {
   *     // Scene changed — re-query
   *   }
   * }
   * ```
   *
   * The iterator ends when the client is closed or the connection drops.
   */
  public frames(): AsyncIterable<VactFrame> & AsyncIterator<VactFrame> {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    return {
      [Symbol.asyncIterator]() {
        return this;
      },
      next(): Promise<IteratorResult<VactFrame>> {
        // If there are buffered frames, emit immediately
        const buffered = self._frameBuffer.shift();
        if (buffered !== undefined) {
          return Promise.resolve({ value: buffered, done: false });
        }
        // If iteration is complete (closed), signal done
        if (self._framingDone) {
          return Promise.resolve({ value: undefined as unknown as VactFrame, done: true });
        }
        // Wait for the next frame
        return new Promise<IteratorResult<VactFrame>>((resolve) => {
          self._frameQueue.push({ resolve });
        });
      },
    };
  }

  // ── Private: handshake ────────────────────────────────────────────────────

  private async _doHandshake(): Promise<void> {
    const capabilities = [
      ...DEFAULT_CAPABILITIES,
      ...this._options.extraCapabilities,
    ];

    // 1. Attach listener BEFORE sending so we never miss an immediate reply
    const ackPromise = new Promise<void>((resolve, reject) => {
      const onMsg = (msg: ServerMessage): void => {
        if (isHandshakeAck(msg)) {
          clearTimeout(timer);
          this._transport.off('message', onMsg);
          resolve();
        }
      };

      const timer = setTimeout(() => {
        this._transport.off('message', onMsg);
        reject(
          new VactTimeoutError('handshake', this._options.handshakeTimeoutMs),
        );
      }, this._options.handshakeTimeoutMs);

      this._transport.on('message', onMsg);
    });

    // 2. Send handshake message to daemon
    await this._transport.send({
      type: 'HANDSHAKE',
      protocol: PROTOCOL_VERSION,
      capabilities,
    });

    // 3. Await the acknowledgement
    await ackPromise;
  }

  // ── Private: message routing ──────────────────────────────────────────────

  private _onMessage(msg: ServerMessage): void {
    if (isSnapshot(msg)) {
      this._handleSnapshot(msg);
    } else if (isDiff(msg)) {
      this._handleDiff(msg);
    } else if (isActionResult(msg)) {
      this._handleActionResult(msg);
    }
    // HANDSHAKE_ACK handled in _doHandshake via temporary listener
  }

  private _handleSnapshot(msg: SnapshotMessage): void {
    const graph: SceneGraph = 'graph' in msg ? (msg as { graph: SceneGraph }).graph : (msg as unknown as SceneGraph);
    this._state.loadSnapshot(graph);
    this.emit('snapshot', graph);
    this._pushFrame({ type: 'SNAPSHOT', graph });
  }

  private _handleDiff(msg: DiffMessage): void {
    if (!this._state.isHydrated) {
      // Waiting for snapshot before diffs can be applied
      return;
    }
    const diff: DiffFrame = 'diff' in msg ? (msg as { diff: DiffFrame }).diff : (msg as unknown as DiffFrame);
    try {
      this._state.applyDiff(diff);
    } catch (err) {
      this._state.reset();
      this.emit('error', err instanceof Error ? err : new Error(String(err)));
      return;
    }
    this.emit('diff', diff);
    this._pushFrame({ type: 'DIFF', diff });
  }

  private _handleActionResult(msg: ActionResultMessage): void {
    const key = `${msg.action}:${msg.target_id}`;
    const waiter = this._pendingActions.get(key);
    if (waiter) {
      clearTimeout(waiter.timer);
      this._pendingActions.delete(key);
      waiter.resolve(msg);
    }
  }

  // ── Private: transport events ─────────────────────────────────────────────

  private _onClose(reason: string): void {
    this._clientState = 'disconnected';
    this._state.reset();
    this._framingDone = true;
    for (const waiter of this._frameQueue) {
      waiter.resolve({ value: undefined as unknown as VactFrame, done: true });
    }
    this._frameQueue = [];
    this.emit('close', reason);
    this.emit('disconnected', reason);
  }

  private _onTransportError(err: Error): void {
    this.emit('error', err);
  }

  // ── Private: async iterator frame push ────────────────────────────────────

  private _pushFrame(frame: VactFrame): void {
    const waiter = this._frameQueue.shift();
    if (waiter) {
      waiter.resolve({ value: frame, done: false });
    } else {
      this._frameBuffer.push(frame);
    }
  }
}
