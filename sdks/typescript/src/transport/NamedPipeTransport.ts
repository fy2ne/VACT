/**
 * NamedPipeTransport — Windows Named Pipe IPC transport for the VACT protocol.
 *
 * Connects to the `vactd` daemon via the Named Pipe at `\\.\pipe\VACT` (or a
 * custom path). Handles framing via {@link FrameDecoder}, reconnection logic,
 * and proper lifecycle management.
 *
 * Platform: **Windows only** (Named Pipes are a Win32 API).
 * Node.js represents Named Pipes as `net.Socket` with a path string — no
 * additional native modules required.
 */

import * as net from 'net';
import EventEmitter from 'eventemitter3';

import type { ITransport, TransportEvents, TransportState } from './ITransport.js';
import type { ServerMessage, ClientMessage } from '../types/index.js';
import { VactConnectionError, VactProtocolError } from '../errors/VactError.js';
import { FrameDecoder, encodeFrame } from './framing.js';

// ─────────────────────────────────────────────────────────────────────────────
// Options
// ─────────────────────────────────────────────────────────────────────────────

export interface NamedPipeTransportOptions {
  /**
   * Full named pipe path.
   * @default `"\\\\.\\pipe\\VACT"`
   */
  pipePath?: string;

  /**
   * Connection timeout in milliseconds.
   * @default 5000
   */
  connectTimeoutMs?: number;
}

// ─────────────────────────────────────────────────────────────────────────────
// Implementation
// ─────────────────────────────────────────────────────────────────────────────

/** Default named pipe path used by `vactd`. */
export const DEFAULT_PIPE_PATH = '\\\\.\\pipe\\VACT';

/**
 * Production transport implementation using Windows Named Pipes.
 *
 * Named Pipes in Node.js are accessed via `net.createConnection({ path })` —
 * identical API to Unix domain sockets, just with a Win32 path string.
 */
export class NamedPipeTransport
  extends EventEmitter<TransportEvents>
  implements ITransport
{
  private readonly _pipePath: string;
  private readonly _connectTimeoutMs: number;

  private _socket: net.Socket | null = null;
  private _decoder: FrameDecoder | null = null;
  private _state: TransportState = 'disconnected';

  public constructor(options: NamedPipeTransportOptions = {}) {
    super();
    this._pipePath = options.pipePath ?? DEFAULT_PIPE_PATH;
    this._connectTimeoutMs = options.connectTimeoutMs ?? 5_000;
  }

  public get state(): TransportState {
    return this._state;
  }

  // ── connect ────────────────────────────────────────────────────────────────

  public connect(): Promise<void> {
    if (this._state === 'connected') {
      return Promise.resolve();
    }
    if (this._state === 'connecting') {
      return Promise.reject(
        new VactConnectionError(this._pipePath, 'Connection already in progress'),
      );
    }

    this._state = 'connecting';

    return new Promise<void>((resolve, reject) => {
      let isResolved = false;
      let tcpAttempt: net.Socket | null = net.createConnection({ port: 4242, host: '127.0.0.1' });

      const timeout = setTimeout(() => {
        if (!isResolved) {
          isResolved = true;
          this._state = 'disconnected';
          tcpAttempt?.destroy();
          reject(
            new VactConnectionError(
              this._pipePath,
              `Connection to vactd timed out after ${this._connectTimeoutMs}ms. ` +
              'Is vactd running? Run "vactd start" and try again.',
            ),
          );
        }
      }, this._connectTimeoutMs);

      const onConnect = (sock: net.Socket) => {
        if (isResolved) return;
        isResolved = true;
        clearTimeout(timeout);
        this._socket = sock;
        this._state = 'connected';
        this._attachSocketListeners(sock);
        resolve();
      };

      tcpAttempt.once('connect', () => {
        onConnect(tcpAttempt!);
      });

      tcpAttempt.once('error', () => {
        tcpAttempt?.destroy();
        tcpAttempt = null;
        if (isResolved) return;

        // Fallback to Windows Named Pipe
        const pipeSock = net.createConnection({ path: this._pipePath });
        pipeSock.once('connect', () => {
          onConnect(pipeSock);
        });
        pipeSock.once('error', (err) => {
          if (isResolved) return;
          isResolved = true;
          clearTimeout(timeout);
          this._state = 'disconnected';
          reject(
            new VactConnectionError(
              this._pipePath,
              `Failed to connect to vactd at "${this._pipePath}": ${(err as NodeJS.ErrnoException).message}. ` +
              'Ensure vactd is running and the pipe path is correct.',
              err,
            ),
          );
        });
      });
    });
  }

  // ── send ───────────────────────────────────────────────────────────────────

  public send(msg: ClientMessage): Promise<void> {
    if (this._state !== 'connected' || !this._socket) {
      return Promise.reject(
        new VactConnectionError(
          this._pipePath,
          `Cannot send message in state "${this._state}": transport is not connected.`,
        ),
      );
    }

    const frame = encodeFrame(msg);
    const socket = this._socket;

    return new Promise<void>((resolve, reject) => {
      socket.write(frame, (err) => {
        if (err) {
          reject(
            new VactConnectionError(
              this._pipePath,
              `Write to pipe failed: ${err.message}`,
              err,
            ),
          );
        } else {
          resolve();
        }
      });
    });
  }

  // ── close ──────────────────────────────────────────────────────────────────

  public close(): void {
    if (this._state === 'closed') return;
    this._state = 'closed';
    if (this._socket) {
      this._socket.destroy();
      this._socket = null;
    }
    this._decoder?.reset();
    this._decoder = null;
  }

  // ── Private: socket event wiring ───────────────────────────────────────────

  private _attachSocketListeners(socket: net.Socket): void {
    // Create a fresh decoder for this connection
    this._decoder = new FrameDecoder((jsonStr) => {
      this._onRawFrame(jsonStr);
    });

    const decoder = this._decoder;

    socket.on('data', (chunk: Buffer) => {
      try {
        decoder.push(chunk);
      } catch (err) {
        if (err instanceof VactProtocolError) {
          this.emit('error', err);
        } else {
          this.emit('error', new VactProtocolError('Unexpected framing error', undefined, err));
        }
      }
    });

    socket.once('close', () => {
      if (this._state !== 'closed') {
        this._state = 'disconnected';
        this._socket = null;
        this.emit('close', 'Named pipe closed by remote (vactd shutdown or crash)');
      }
    });

    socket.on('error', (err) => {
      if (this._state !== 'closed') {
        this.emit(
          'error',
          new VactConnectionError(
            this._pipePath,
            `Named pipe error: ${err.message}`,
            err,
          ),
        );
      }
    });
  }

  // ── Private: message parsing ───────────────────────────────────────────────

  private _onRawFrame(jsonStr: string): void {
    let parsed: unknown;
    try {
      parsed = JSON.parse(jsonStr) as unknown;
    } catch (err) {
      this.emit(
        'error',
        new VactProtocolError(
          'Failed to parse JSON frame from vactd',
          jsonStr.slice(0, 256), // first 256 chars for diagnostics
          err,
        ),
      );
      return;
    }

    // Validate it has at least a 'type' discriminator
    if (
      typeof parsed !== 'object' ||
      parsed === null ||
      !('type' in parsed) ||
      typeof (parsed as Record<string, unknown>)['type'] !== 'string'
    ) {
      this.emit(
        'error',
        new VactProtocolError(
          'Received IPC frame with missing or non-string "type" field',
          jsonStr.slice(0, 256),
        ),
      );
      return;
    }

    // Trusted cast — validated by daemon, further validation in StateManager
    this.emit('message', parsed as ServerMessage);
  }
}
