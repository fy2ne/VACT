/**
 * StateManager — in-memory scene graph hydration engine.
 *
 * Maintains the current authoritative scene graph by:
 * 1. Loading an initial `SceneGraph` snapshot from the first frame.
 * 2. Applying incoming `DiffFrame` mutations incrementally.
 *
 * This gives agents a single `getState()` call that always returns the
 * latest complete scene graph — no manual diff-application needed.
 *
 * Thread safety: Node.js is single-threaded, so no locking is required.
 */

import type { SceneGraph, SceneNode } from '../types/scene.js';
import type {
  DiffFrame,
  Mutation,
  NodePatch,
  InsertMutation,
  UpdateMutation,
} from '../types/diff.js';
import { VactStateError } from '../errors/VactError.js';

// ─────────────────────────────────────────────────────────────────────────────
// StateManager
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Stateful scene graph maintainer.
 *
 * Feed it snapshots and diffs; query the current state at any time via
 * {@link getState}.
 */
export class StateManager {
  private _current: SceneGraph | null = null;

  /** `true` after the first snapshot has been loaded. */
  public get isHydrated(): boolean {
    return this._current !== null;
  }

  /**
   * Get the current scene graph.
   *
   * @throws {VactStateError} if no snapshot has been received yet.
   */
  public getState(): SceneGraph {
    if (!this._current) {
      throw new VactStateError(
        'Scene graph is not yet hydrated. Wait for the "snapshot" event before querying state.',
      );
    }
    return this._current;
  }

  /**
   * Load an initial full snapshot (or reset state after a desktop switch).
   *
   * Replaces any existing state entirely.
   */
  public loadSnapshot(graph: SceneGraph): void {
    this._current = graph;
  }

  /**
   * Apply an incremental diff to the current scene graph.
   *
   * @throws {VactStateError} if called before a snapshot has been loaded.
   * @throws {VactStateError} if sequence numbers are inconsistent
   *   (a gap indicates a missed frame — reset and re-subscribe).
   */
  public applyDiff(diff: DiffFrame): void {
    if (!this._current) {
      throw new VactStateError(
        'Cannot apply diff: no snapshot loaded. The connection may have missed the initial snapshot.',
      );
    }

    if (diff.ack_seq !== this._current.seq) {
      // Sequence mismatch — we missed a frame. Signal this via the error
      // rather than silently corrupting state.
      throw new VactStateError(
        `Diff sequence mismatch: expected ack_seq=${this._current.seq}, ` +
        `got ack_seq=${diff.ack_seq}. ` +
        'A frame was dropped. The client will re-sync on the next snapshot.',
      );
    }

    if (diff.mutations.length === 0) {
      // Stable frame — update the sequence number only
      this._current = { ...this._current, seq: diff.seq };
      return;
    }

    // Apply all mutations and produce a new root
    const newRoot = applyMutationsToNode(this._current.root, diff.mutations);
    this._current = {
      ...this._current,
      seq: diff.seq,
      root: newRoot,
    };
  }

  /** Reset all state (call on transport close). */
  public reset(): void {
    this._current = null;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Mutation application
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Apply a list of mutations to a scene node subtree, returning a new immutable
 * root node. Mutations are processed in order.
 *
 * Returns a new node tree — the original is not mutated.
 */
function applyMutationsToNode(
  root: SceneNode,
  mutations: readonly Mutation[],
): SceneNode {
  let current = root;
  for (const mutation of mutations) {
    current = applyOneMutation(current, mutation);
  }
  return current;
}

function applyOneMutation(root: SceneNode, mutation: Mutation): SceneNode {
  switch (mutation.op) {
    case 'INSERT':
      return applyInsert(root, mutation);
    case 'REMOVE':
      return applyRemove(root, mutation.id);
    case 'UPDATE':
      return applyUpdate(root, mutation);
  }
}

function applyInsert(node: SceneNode, mutation: InsertMutation): SceneNode {
  if (node.id === mutation.parent_id) {
    // Insert as a new child of this node
    const existingChildren = node.children ?? [];
    return {
      ...node,
      children: [...existingChildren, mutation.node],
    };
  }
  // Recurse into children
  const children = node.children;
  if (!children || children.length === 0) return node;

  const newChildren = children.map((child) => applyInsert(child, mutation));
  // Short-circuit if nothing changed (avoid unnecessary object creation)
  if (newChildren.every((c, i) => c === children[i])) return node;
  return { ...node, children: newChildren };
}

function applyRemove(node: SceneNode, targetId: number): SceneNode {
  const children = node.children;
  if (!children || children.length === 0) return node;

  const filtered = children.filter((c) => c.id !== targetId);
  if (filtered.length === children.length) {
    // Not a direct child — recurse
    const newChildren = filtered.map((c) => applyRemove(c, targetId));
    if (newChildren.every((c, i) => c === filtered[i])) return node;
    return { ...node, children: newChildren };
  }
  return { ...node, children: filtered };
}

function applyUpdate(node: SceneNode, mutation: UpdateMutation): SceneNode {
  if (node.id === mutation.id) {
    return mergeNodePatch(node, mutation.patch);
  }
  const children = node.children;
  if (!children || children.length === 0) return node;

  const newChildren = children.map((c) => applyUpdate(c, mutation));
  if (newChildren.every((c, i) => c === children[i])) return node;
  return { ...node, children: newChildren };
}

function mergeNodePatch(node: SceneNode, patch: NodePatch): SceneNode {
  return {
    ...node,
    ...(patch.bounds !== undefined ? { bounds: patch.bounds } : {}),
    ...(patch.type !== undefined ? { type: patch.type } : {}),
    ...(patch.label !== undefined ? { label: patch.label } : {}),
    ...(patch.value !== undefined ? { value: patch.value } : {}),
    ...(patch.focused !== undefined ? { focused: patch.focused } : {}),
    ...(patch.interactable !== undefined ? { interactable: patch.interactable } : {}),
    ...(patch.disabled !== undefined ? { disabled: patch.disabled } : {}),
  };
}
