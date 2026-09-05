/**
 * @module @vact/types
 * Public type exports for the VACT/1.0 wire protocol.
 */

// Scene graph
export type {
  NodeType,
  Viewport,
  Bounds,
  SceneNode,
  SceneGraph,
} from './scene.js';
export { boundsToRect, boundsCenter } from './scene.js';

// Temporal diffs
export type {
  NodePatch,
  InsertMutation,
  RemoveMutation,
  UpdateMutation,
  Mutation,
  DiffFrame,
} from './diff.js';
export { isInsert, isRemove, isUpdate } from './diff.js';

// IPC messages
export type {
  ActionKind,
  HandshakeMessage,
  HandshakeAckMessage,
  SnapshotMessage,
  DiffMessage,
  ActionMessage,
  ActionResultMessage,
  ClientMessage,
  ServerMessage,
  IpcMessage,
} from './ipc.js';
export {
  isHandshakeAck,
  isSnapshot,
  isDiff,
  isActionResult,
  PROTOCOL_VERSION,
  DEFAULT_CAPABILITIES,
} from './ipc.js';
