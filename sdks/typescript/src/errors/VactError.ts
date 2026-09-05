/**
 * Typed error hierarchy for the VACT SDK.
 *
 * All SDK errors extend `VactError` so consumers can catch them uniformly:
 *
 * ```typescript
 * try {
 *   await client.connect();
 * } catch (err) {
 *   if (err instanceof VactConnectionError) {
 *     // daemon not running, pipe not found
 *   } else if (err instanceof VactProtocolError) {
 *     // framing or JSON parse failure
 *   }
 * }
 * ```
 */

// ─────────────────────────────────────────────────────────────────────────────
// Base error
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Base class for all VACT SDK errors.
 *
 * Always carries a `code` string for programmatic matching, in addition to
 * the human-readable `message`.
 */
export class VactError extends Error {
  public readonly code: string;
  public readonly cause?: unknown;

  public constructor(code: string, message: string, cause?: unknown) {
    super(message);
    this.name = 'VactError';
    this.code = code;
    if (cause !== undefined) {
      this.cause = cause;
    }
    // Maintain proper stack trace in V8
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, new.target);
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Connection errors
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Thrown when the SDK cannot establish or maintain a connection to `vactd`.
 *
 * Common causes:
 * - `vactd` is not running
 * - Named pipe path is incorrect
 * - Insufficient permissions to open the pipe
 * - Daemon crashed mid-session
 */
export class VactConnectionError extends VactError {
  /** Named pipe path that was attempted. */
  public readonly pipePath: string;

  public constructor(pipePath: string, message: string, cause?: unknown) {
    super('VACT_CONNECTION_ERROR', message, cause);
    this.name = 'VactConnectionError';
    this.pipePath = pipePath;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Protocol errors
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Thrown when the wire protocol is violated — malformed framing, invalid JSON,
 * unexpected message type, or protocol version mismatch.
 */
export class VactProtocolError extends VactError {
  /** Raw bytes or string that caused the parse failure, if available. */
  public readonly rawPayload?: string | undefined;

  public constructor(message: string, rawPayload?: string, cause?: unknown) {
    super('VACT_PROTOCOL_ERROR', message, cause);
    this.name = 'VactProtocolError';
    if (rawPayload !== undefined) {
      this.rawPayload = rawPayload;
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Timeout errors
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Thrown when an operation (handshake, action dispatch) exceeds the configured
 * timeout without completing.
 */
export class VactTimeoutError extends VactError {
  /** The operation that timed out (e.g. `"handshake"`, `"action:CLICK"`). */
  public readonly operation: string;
  /** Timeout duration in milliseconds. */
  public readonly timeoutMs: number;

  public constructor(operation: string, timeoutMs: number) {
    super(
      'VACT_TIMEOUT_ERROR',
      `Operation "${operation}" timed out after ${timeoutMs}ms`,
    );
    this.name = 'VactTimeoutError';
    this.operation = operation;
    this.timeoutMs = timeoutMs;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Action errors
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Thrown when `vactd` rejects an action dispatch.
 *
 * This typically means the target node ID is stale (the element was removed
 * from the scene between when the agent queried it and when it sent the action).
 */
export class VactActionError extends VactError {
  /** The action kind that was rejected (e.g. `"CLICK"`). */
  public readonly action: string;
  /** The target node ID that was specified. */
  public readonly targetId: number;
  /** Error message returned by the daemon. */
  public readonly daemonError: string;

  public constructor(action: string, targetId: number, daemonError: string) {
    super(
      'VACT_ACTION_ERROR',
      `Action "${action}" on node ${targetId} rejected by daemon: ${daemonError}`,
    );
    this.name = 'VactActionError';
    this.action = action;
    this.targetId = targetId;
    this.daemonError = daemonError;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// State errors
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Thrown when an operation requires an established, hydrated scene state but
 * the client is not yet connected or no snapshot has been received.
 */
export class VactStateError extends VactError {
  public constructor(message: string) {
    super('VACT_STATE_ERROR', message);
    this.name = 'VactStateError';
  }
}
