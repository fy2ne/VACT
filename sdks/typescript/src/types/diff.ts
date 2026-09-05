/**
 * VACT/1.0 Temporal Delta Diff Types
 *
 * Mirrors the Rust `vact-protocol::diff` module. These types represent the
 * minimal mutation stream emitted between consecutive scene graph frames —
 * the core mechanism that keeps VACT bandwidth under 2 KB/s.
 *
 * @see {@link https://vact.fy2ne.me/spec/diff-protocol}
 */

import type { NodeType, SceneNode, Bounds } from './scene.js';

// ─────────────────────────────────────────────────────────────────────────────
// NodePatch
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Field-level patch applied by an `UPDATE` mutation.
 *
 * Only fields that have **changed** since the previous frame are present.
 * All absent fields are unchanged and should be preserved in local state.
 */
export interface NodePatch {
  /** New bounding rect `[x1, y1, x2, y2]`, if changed. */
  readonly bounds?: Bounds;
  /** New semantic type, if changed. */
  readonly type?: NodeType;
  /** New label text, if changed. */
  readonly label?: string;
  /** New input value, if changed. */
  readonly value?: string;
  /** New focus state, if changed. */
  readonly focused?: boolean;
  /** New interactable flag, if changed. */
  readonly interactable?: boolean;
  /** New disabled flag, if changed. */
  readonly disabled?: boolean;
}

// ─────────────────────────────────────────────────────────────────────────────
// Mutation (discriminated union)
// ─────────────────────────────────────────────────────────────────────────────

/**
 * A new node was inserted into the scene graph.
 *
 * The `node` includes the complete subtree rooted at this insertion point.
 */
export interface InsertMutation {
  readonly op: 'INSERT';
  /** Stable ID of the parent node. `0` = virtual root. */
  readonly parent_id: number;
  /** The new node and its full subtree. */
  readonly node: SceneNode;
}

/**
 * An existing node (and all its descendants) was removed from the scene graph.
 */
export interface RemoveMutation {
  readonly op: 'REMOVE';
  /** Stable ID of the removed node. */
  readonly id: number;
}

/**
 * An existing node's properties changed.
 *
 * Only changed fields are present in `patch`; all others remain unchanged.
 * Child insertions/removals are reported as separate `INSERT`/`REMOVE` mutations.
 */
export interface UpdateMutation {
  readonly op: 'UPDATE';
  /** Stable ID of the modified node. */
  readonly id: number;
  /** Field-level patch containing only changed fields. */
  readonly patch: NodePatch;
}

/** Discriminated union of all possible scene graph mutations. */
export type Mutation = InsertMutation | RemoveMutation | UpdateMutation;

// ─────────────────────────────────────────────────────────────────────────────
// DiffFrame
// ─────────────────────────────────────────────────────────────────────────────

/**
 * A temporal delta frame — the minimal set of mutations between two consecutive
 * scene graph snapshots.
 *
 * Payloads are consistently `< 2 KB` during typical UI interaction, reducing
 * agent perceptual context consumption by **98.5%** vs. screenshot-based systems.
 *
 * @example
 * ```json
 * {
 *   "type": "DIFF",
 *   "seq": 1043,
 *   "ack_seq": 1042,
 *   "mutations": [
 *     { "op": "UPDATE", "id": 14, "patch": { "value": "Deploy complete." } },
 *     { "op": "UPDATE", "id": 18, "patch": { "disabled": false } }
 *   ]
 * }
 * ```
 */
export interface DiffFrame {
  /** Message discriminant. Always `"DIFF"`. */
  readonly type: 'DIFF';
  /** Monotonically increasing sequence number for this diff. */
  readonly seq: number;
  /** Sequence number of the previous snapshot this diff was computed against. */
  readonly ack_seq: number;
  /**
   * Ordered list of scene mutations. May be empty if the scene did not change
   * between frames (stable frame).
   */
  readonly mutations: readonly Mutation[];
}

// ─────────────────────────────────────────────────────────────────────────────
// Type guards
// ─────────────────────────────────────────────────────────────────────────────

/** Type guard — narrows a `Mutation` to `InsertMutation`. */
export function isInsert(m: Mutation): m is InsertMutation {
  return m.op === 'INSERT';
}

/** Type guard — narrows a `Mutation` to `RemoveMutation`. */
export function isRemove(m: Mutation): m is RemoveMutation {
  return m.op === 'REMOVE';
}

/** Type guard — narrows a `Mutation` to `UpdateMutation`. */
export function isUpdate(m: Mutation): m is UpdateMutation {
  return m.op === 'UPDATE';
}
