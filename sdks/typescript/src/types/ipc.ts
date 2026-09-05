/**
 * VACT/1.0 IPC Message Types
 *
 * Defines the complete discriminated union of all messages exchanged over the
 * Named Pipe IPC transport between `vactd` and SDK clients.
 *
 * Wire framing: 4-byte little-endian length prefix + UTF-8 JSON payload.
 * Discriminator: `"type"` field using SCREAMING_SNAKE_CASE values.
 *
 * @see {@link https://vact.fy2ne.me/spec/ipc}
 */

import type { SceneGraph, Viewport } from './scene.js';
import type { DiffFrame } from './diff.js';

// ─────────────────────────────────────────────────────────────────────────────
// Client → Server messages
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Initial handshake sent by the client after connecting.
 *
 * `capabilities` lists optional features the client supports, allowing the
 * daemon to tailor its output (e.g. `"DIFF"`, `"SNAPSHOT_ON_DEMAND"`).
 */
export interface HandshakeMessage {
  readonly type: 'HANDSHAKE';
  readonly protocol: string;
  readonly capabilities: readonly string[];
}

// ─────────────────────────────────────────────────────────────────────────────
// Server → Client messages
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Handshake acknowledgement sent by the daemon after a successful handshake.
 *
 * Contains the current viewport so clients can initialize their layout state
 * before the first snapshot arrives.
 */
export interface HandshakeAckMessage {
  readonly type: 'HANDSHAKE_ACK';
  readonly viewport: Viewport;
}

/**
 * Full scene graph snapshot.
 *
 * Sent on initial connection and whenever a full state reset is triggered.
 * Wire format serializes SceneGraph fields directly alongside `type: "SNAPSHOT"`.
 */
export type SnapshotMessage = (SceneGraph & { readonly type: 'SNAPSHOT' }) | {
  readonly type: 'SNAPSHOT';
  readonly graph: SceneGraph;
};

/**
 * Temporal delta diff message.
 *
 * Sent on every subsequent frame containing scene mutations.
 * Wire format serializes DiffFrame fields directly alongside `type: "DIFF"`.
 */
export type DiffMessage = (DiffFrame & { readonly type: 'DIFF' }) | {
  readonly type: 'DIFF';
  readonly diff: DiffFrame;
};

// ─────────────────────────────────────────────────────────────────────────────
// Client → Server action messages
// ─────────────────────────────────────────────────────────────────────────────

/** All valid action verbs for the VACT I/O bus. */
export type ActionKind = 'CLICK' | 'TYPE' | 'SCROLL' | 'KEY' | 'FOCUS' | 'LEARN';

/**
 * Agent action dispatched to the I/O bus.
 *
 * The daemon resolves `target_id` to pixel coordinates via the scene graph
 * and routes the action through Direct Mode (`SendInput`) or Ghost Mode
 * (`PostMessageW`) based on the target window type.
 */
export interface ActionMessage {
  readonly type: 'ACTION';
  /** The action to perform. */
  readonly action: ActionKind;
  /** Stable scene node ID to target. */
  readonly target_id: number;
  /** Text payload for `TYPE` actions. */
  readonly text?: string;
  /** Scroll delta in logical pixels for `SCROLL` actions (negative = up). */
  readonly delta_y?: number;
  /** Virtual key code for `KEY` actions (Windows VK_ constants). */
  readonly vk?: number;
  /** Semantic label for `LEARN` actions. */
  readonly label?: string;
  /** Action result outcome string for `LEARN` actions. */
  readonly action_result?: string;
}

// ─────────────────────────────────────────────────────────────────────────────
// Server → Client action result
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Result of an action dispatched via {@link ActionMessage}.
 *
 * `ok = true` means the action was dispatched to the OS input queue.
 * `ok = false` means the action was rejected; `error` contains the reason.
 */
export interface ActionResultMessage {
  readonly type: 'ACTION_RESULT';
  readonly action: ActionKind;
  readonly target_id: number;
  /** `"DIRECT"` (SendInput) or `"GHOST"` (PostMessageW). */
  readonly route: 'DIRECT' | 'GHOST';
  readonly ok: boolean;
  readonly error?: string;
}

// ─────────────────────────────────────────────────────────────────────────────
// Union types
// ─────────────────────────────────────────────────────────────────────────────

/** Any message that can be sent from the SDK client to `vactd`. */
export type ClientMessage = HandshakeMessage | ActionMessage;

/** Any message that can be received from `vactd` by the SDK client. */
export type ServerMessage =
  | HandshakeAckMessage
  | SnapshotMessage
  | DiffMessage
  | ActionResultMessage;

/** Combined union of all IPC messages (useful for generic transport layers). */
export type IpcMessage = ClientMessage | ServerMessage;

// ─────────────────────────────────────────────────────────────────────────────
// Type guards for server messages
// ─────────────────────────────────────────────────────────────────────────────

/** Narrows a `ServerMessage` to `HandshakeAckMessage`. */
export function isHandshakeAck(m: ServerMessage): m is HandshakeAckMessage {
  return m.type === 'HANDSHAKE_ACK';
}

/** Narrows a `ServerMessage` to `SnapshotMessage`. */
export function isSnapshot(m: ServerMessage): m is SnapshotMessage {
  return m.type === 'SNAPSHOT';
}

/** Narrows a `ServerMessage` to `DiffMessage`. */
export function isDiff(m: ServerMessage): m is DiffMessage {
  return m.type === 'DIFF';
}

/** Narrows a `ServerMessage` to `ActionResultMessage`. */
export function isActionResult(m: ServerMessage): m is ActionResultMessage {
  return m.type === 'ACTION_RESULT';
}

// ─────────────────────────────────────────────────────────────────────────────
// Protocol constants
// ─────────────────────────────────────────────────────────────────────────────

/** The VACT wire protocol version string sent in every handshake. */
export const PROTOCOL_VERSION = 'VACT/1.0' as const;

/** Default capabilities advertised by this SDK version. */
export const DEFAULT_CAPABILITIES: readonly string[] = [
  'DIFF',
  'SNAPSHOT',
  'ACTION',
  'ACTION_RESULT',
] as const;
