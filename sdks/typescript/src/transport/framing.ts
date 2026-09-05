/**
 * VACT wire-framing codec.
 *
 * The VACT IPC transport uses a simple, robust binary framing protocol:
 *
 * ```
 * ┌────────────────────┬─────────────────────────────────┐
 * │  Length (4 bytes)  │       JSON Payload (N bytes)    │
 * │  Little-Endian u32 │  UTF-8 encoded JSON string      │
 * └────────────────────┴─────────────────────────────────┘
 * ```
 *
 * This codec is stateful — it accumulates incoming bytes and emits complete
 * JSON payloads only once a full frame has been received.
 */

import { VactProtocolError } from '../errors/VactError.js';

/** Maximum allowed frame payload size (16 MB). Guards against memory exhaustion. */
const MAX_FRAME_BYTES = 16 * 1024 * 1024;

/** Size of the length prefix header in bytes. */
const HEADER_SIZE = 4;

// ─────────────────────────────────────────────────────────────────────────────
// Encode (client → wire)
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Encode a JavaScript object into a length-prefixed JSON frame.
 *
 * @param payload - Any JSON-serializable value
 * @returns `Buffer` containing the 4-byte LE length header followed by the UTF-8 JSON body
 */
export function encodeFrame(payload: unknown): Buffer {
  const json = JSON.stringify(payload);
  const body = Buffer.from(json, 'utf8');
  const frame = Buffer.allocUnsafe(HEADER_SIZE + body.byteLength);
  frame.writeUInt32LE(body.byteLength, 0);
  body.copy(frame, HEADER_SIZE);
  return frame;
}

// ─────────────────────────────────────────────────────────────────────────────
// Decoder (wire → JavaScript)
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Stateful streaming decoder for the VACT length-prefix framing protocol.
 *
 * Feed raw incoming bytes via {@link push} and receive complete decoded frames
 * via the `onFrame` callback.
 *
 * @example
 * ```typescript
 * const decoder = new FrameDecoder((payload) => {
 *   const msg = JSON.parse(payload) as ServerMessage;
 *   handleMessage(msg);
 * });
 *
 * socket.on('data', (chunk) => decoder.push(chunk));
 * ```
 */
export class FrameDecoder {
  private _buffer: Buffer = Buffer.alloc(0);
  private readonly _onFrame: (jsonPayload: string) => void;

  /**
   * @param onFrame - Callback invoked with the UTF-8 JSON string of each complete frame.
   *   Called synchronously within {@link push} — do not block.
   */
  public constructor(onFrame: (jsonPayload: string) => void) {
    this._onFrame = onFrame;
  }

  /**
   * Feed raw bytes into the decoder.
   *
   * May emit zero, one, or multiple frames if the incoming chunk contains
   * multiple complete messages (TCP/pipe coalescing).
   *
   * @throws {VactProtocolError} if a frame exceeds {@link MAX_FRAME_BYTES}
   *   or if the JSON payload is malformed.
   */
  public push(chunk: Buffer): void {
    // Append incoming bytes to the internal buffer
    this._buffer = Buffer.concat([this._buffer, chunk]);

    // Drain as many complete frames as possible
    while (this._buffer.byteLength >= HEADER_SIZE) {
      const payloadLen = this._buffer.readUInt32LE(0);

      if (payloadLen > MAX_FRAME_BYTES) {
        this.reset();
        throw new VactProtocolError(
          `Frame size ${payloadLen} exceeds maximum allowed ${MAX_FRAME_BYTES} bytes — ` +
          'possible protocol corruption or rogue sender.',
        );
      }

      const totalLen = HEADER_SIZE + payloadLen;
      if (this._buffer.byteLength < totalLen) {
        // Incomplete frame — wait for more data
        break;
      }

      // Extract the payload
      const payloadBuf = this._buffer.subarray(HEADER_SIZE, totalLen);
      const jsonStr = payloadBuf.toString('utf8');

      // Advance buffer past this frame
      this._buffer = this._buffer.subarray(totalLen);

      // Emit the decoded frame
      this._onFrame(jsonStr);
    }
  }

  /**
   * Reset internal buffer state (call on connection close/error).
   */
  public reset(): void {
    this._buffer = Buffer.alloc(0);
  }

  /** Number of bytes currently buffered (useful for diagnostics). */
  public get bufferedBytes(): number {
    return this._buffer.byteLength;
  }
}
