/**
 * ITransport — abstract transport interface.
 *
 * Decouples the client logic from the physical transport mechanism.
 * The production implementation uses Windows Named Pipes.
 * Test implementations can inject mock transports without any OS dependencies.
 */

import type EventEmitter from 'eventemitter3';
import type { ServerMessage, ClientMessage } from '../types/index.js';

// ─────────────────────────────────────────────────────────────────────────────
// Transport lifecycle state
// ─────────────────────────────────────────────────────────────────────────────

export type TransportState = 'disconnected' | 'connecting' | 'connected' | 'closed';

// ─────────────────────────────────────────────────────────────────────────────
// Transport event map
// ─────────────────────────────────────────────────────────────────────────────

export interface TransportEvents {
  /** A fully framed, parsed server message has arrived. */
  message: (msg: ServerMessage) => void;
  /** The transport connection was lost (may be retryable). */
  close: (reason: string) => void;
  /** An unrecoverable transport-level error occurred. */
  error: (err: Error) => void;
}

// ─────────────────────────────────────────────────────────────────────────────
// ITransport interface
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Abstract transport contract for the VACT IPC layer.
 *
 * Extends `EventEmitter<TransportEvents>` so all standard listener methods
 * (`on`, `off`, `once`, `emit`) are typed consistently.
 *
 * Implementors handle:
 * 1. Physical connection establishment
 * 2. 4-byte LE length-prefix framing (encode on write, decode on read)
 * 3. JSON serialization / deserialization
 * 4. Lifecycle management (connect → connected → close)
 */
export interface ITransport extends EventEmitter<TransportEvents> {
  /** Current lifecycle state of this transport. */
  readonly state: TransportState;

  /**
   * Establish the physical connection and start the read loop.
   *
   * Resolves when the connection is open and ready to send/receive.
   * Rejects with {@link VactConnectionError} if the connection fails.
   */
  connect(): Promise<void>;

  /**
   * Send a client message to `vactd`.
   *
   * Serializes `msg` to JSON, prepends the 4-byte LE length header, and
   * writes the frame to the underlying channel.
   *
   * @throws {VactConnectionError} if the transport is not in `"connected"` state.
   */
  send(msg: ClientMessage): Promise<void>;

  /**
   * Close the transport gracefully.
   *
   * After calling `close()`, no further messages will be emitted and all
   * pending operations will be cancelled.
   */
  close(): void;
}
