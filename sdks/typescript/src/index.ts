/**
 * vact-sdk — Enterprise TypeScript/Node.js SDK for the VACT Protocol
 *
 * VACT (Vector Agent Context Transport) is a hardware-accelerated, real-time
 * vector scene graph streaming protocol for autonomous AI agents.
 *
 * ## Quick Start
 *
 * ```typescript
 * import { VactClient } from 'vact-sdk';
 *
 * const client = new VactClient();
 * await client.connect();
 *
 * // Event-driven: react to scene changes
 * client.on('snapshot', (graph) => {
 *   console.log('Active window:', graph.active_window);
 * });
 *
 * client.on('diff', (diff) => {
 *   console.log(`${diff.mutations.length} mutations in frame ${diff.seq}`);
 * });
 *
 * // Query the live scene graph
 * const sendButton = client.query()
 *   .ofType('BUTTON')
 *   .withLabel(/send/i)
 *   .first();
 *
 * if (sendButton) {
 *   await client.agent().click(sendButton.id);
 * }
 *
 * // Or: use async iteration (for agent loop patterns)
 * for await (const frame of client.frames()) {
 *   if (frame.type === 'DIFF') { ... }
 * }
 * ```
 *
 * @see {@link https://vact.fy2ne.me/docs/sdk/typescript}
 * @author fy2ne
 * @license MIT
 */

// ─── Client ──────────────────────────────────────────────────────────────────
export { VactClient } from './client/VactClient.js';
export type { ClientState, VactClientEvents, VactFrame } from './client/VactClient.js';
export type { VactClientOptions } from './client/VactClientOptions.js';
export { StateManager } from './client/StateManager.js';

// ─── Query ───────────────────────────────────────────────────────────────────
export { SceneQuery } from './query/SceneQuery.js';

// ─── Agent ───────────────────────────────────────────────────────────────────
export { AgentContext } from './agent/AgentContext.js';

// ─── Transport ───────────────────────────────────────────────────────────────
export { NamedPipeTransport, DEFAULT_PIPE_PATH } from './transport/NamedPipeTransport.js';
export type {
  ITransport,
  TransportEvents,
  TransportState,
} from './transport/ITransport.js';
export type { NamedPipeTransportOptions } from './transport/NamedPipeTransport.js';
export { FrameDecoder, encodeFrame } from './transport/framing.js';

// ─── Types ───────────────────────────────────────────────────────────────────
export type {
  NodeType,
  Viewport,
  Bounds,
  SceneNode,
  SceneGraph,
} from './types/scene.js';
export { boundsToRect, boundsCenter } from './types/scene.js';

export type {
  NodePatch,
  InsertMutation,
  RemoveMutation,
  UpdateMutation,
  Mutation,
  DiffFrame,
} from './types/diff.js';
export { isInsert, isRemove, isUpdate } from './types/diff.js';

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
} from './types/ipc.js';
export {
  isHandshakeAck,
  isSnapshot,
  isDiff,
  isActionResult,
  PROTOCOL_VERSION,
  DEFAULT_CAPABILITIES,
} from './types/ipc.js';

// ─── Errors ───────────────────────────────────────────────────────────────────
export {
  VactError,
  VactConnectionError,
  VactProtocolError,
  VactTimeoutError,
  VactActionError,
  VactStateError,
} from './errors/VactError.js';
