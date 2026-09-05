/**
 * VactClientOptions — Configuration for the VACT client.
 *
 * Pass an options object to `new VactClient(options)`.
 * All fields are optional — sensible defaults are provided for every setting.
 */

import type { ITransport } from '../transport/ITransport.js';
import { NamedPipeTransport, DEFAULT_PIPE_PATH } from '../transport/NamedPipeTransport.js';

// ─────────────────────────────────────────────────────────────────────────────
// Options interface
// ─────────────────────────────────────────────────────────────────────────────

export interface VactClientOptions {
  /**
   * Full named pipe path to `vactd`.
   *
   * Override this if you ran `vactd` with a custom `--pipe` argument.
   *
   * @default `"\\\\.\\pipe\\VACT"`
   */
  pipePath?: string;

  /**
   * Custom transport implementation. Overrides `pipePath` when provided.
   *
   * Use this for testing (inject a `MockTransport`) or for alternative
   * transports (e.g. TCP socket for remote sessions).
   *
   * @example
   * ```typescript
   * const client = new VactClient({ transport: new MockTransport() });
   * ```
   */
  transport?: ITransport;

  /**
   * Handshake timeout in milliseconds.
   *
   * After the Named Pipe connects, the client sends a `HANDSHAKE` message and
   * waits for `HANDSHAKE_ACK`. If the daemon does not respond within this
   * timeout, a {@link VactTimeoutError} is thrown.
   *
   * @default 5000
   */
  handshakeTimeoutMs?: number;

  /**
   * Action dispatch timeout in milliseconds.
   *
   * Maximum time to wait for an `ACTION_RESULT` from `vactd` after sending
   * an `ACTION` message.
   *
   * @default 3000
   */
  actionTimeoutMs?: number;

  /**
   * Additional protocol capabilities to advertise in the handshake.
   *
   * The SDK always advertises its default capabilities. Use this to announce
   * custom extensions.
   *
   * @default `[]`
   */
  extraCapabilities?: readonly string[];
}

// ─────────────────────────────────────────────────────────────────────────────
// Resolved options (all fields required)
// ─────────────────────────────────────────────────────────────────────────────

export interface ResolvedVactClientOptions {
  transport: ITransport;
  handshakeTimeoutMs: number;
  actionTimeoutMs: number;
  extraCapabilities: readonly string[];
}

/**
 * Resolve user-provided options into a complete config object.
 * Called once in the `VactClient` constructor.
 */
export function resolveOptions(opts: VactClientOptions = {}): ResolvedVactClientOptions {
  const transport =
    opts.transport ??
    new NamedPipeTransport({ pipePath: opts.pipePath ?? DEFAULT_PIPE_PATH });

  return {
    transport,
    handshakeTimeoutMs: opts.handshakeTimeoutMs ?? 5_000,
    actionTimeoutMs: opts.actionTimeoutMs ?? 3_000,
    extraCapabilities: opts.extraCapabilities ?? [],
  };
}
