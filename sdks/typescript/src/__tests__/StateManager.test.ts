/**
 * Tests for StateManager — scene graph hydration and diff application.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { StateManager } from '../client/StateManager.js';
import { VactStateError } from '../errors/VactError.js';
import type { SceneGraph, SceneNode } from '../types/scene.js';
import type { DiffFrame } from '../types/diff.js';

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

function makeGraph(seq: number, root: SceneNode): SceneGraph {
  return {
    protocol: 'VACT/1.0',
    seq,
    timestamp_us: seq * 16_000,
    viewport: { width: 1920, height: 1080, scale_factor: 1.0 },
    active_window: 'TestApp',
    root,
  };
}

function makeNode(id: number, type: SceneNode['type'] = 'CONTAINER', children?: SceneNode[]): SceneNode {
  return {
    id,
    type,
    bounds: [0, 0, 100, 50],
    ...(children !== undefined ? { children } : {}),
  };
}

function makeDiff(seq: number, ack_seq: number, mutations: DiffFrame['mutations'] = []): DiffFrame {
  return { type: 'DIFF', seq, ack_seq, mutations };
}

// ─────────────────────────────────────────────────────────────────────────────
// Basic lifecycle
// ─────────────────────────────────────────────────────────────────────────────

describe('StateManager — lifecycle', () => {
  let sm: StateManager;
  beforeEach(() => { sm = new StateManager(); });

  it('starts not hydrated', () => {
    expect(sm.isHydrated).toBe(false);
  });

  it('throws VactStateError when getState() called before snapshot', () => {
    expect(() => sm.getState()).toThrow(VactStateError);
  });

  it('becomes hydrated after loadSnapshot', () => {
    sm.loadSnapshot(makeGraph(1, makeNode(1)));
    expect(sm.isHydrated).toBe(true);
  });

  it('resets to not-hydrated after reset()', () => {
    sm.loadSnapshot(makeGraph(1, makeNode(1)));
    sm.reset();
    expect(sm.isHydrated).toBe(false);
    expect(() => sm.getState()).toThrow(VactStateError);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Snapshot loading
// ─────────────────────────────────────────────────────────────────────────────

describe('StateManager — loadSnapshot', () => {
  it('stores the graph exactly', () => {
    const sm = new StateManager();
    const graph = makeGraph(5, makeNode(1, 'BUTTON'));
    sm.loadSnapshot(graph);
    expect(sm.getState()).toBe(graph); // reference equality — no copy
  });

  it('overwrites previous state on second snapshot', () => {
    const sm = new StateManager();
    sm.loadSnapshot(makeGraph(1, makeNode(1)));
    const graph2 = makeGraph(99, makeNode(9, 'TEXT'));
    sm.loadSnapshot(graph2);
    expect(sm.getState().seq).toBe(99);
    expect(sm.getState().root.id).toBe(9);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// applyDiff — stable frame (0 mutations)
// ─────────────────────────────────────────────────────────────────────────────

describe('StateManager — applyDiff (stable frame)', () => {
  it('advances sequence number without changing tree', () => {
    const sm = new StateManager();
    const root = makeNode(1, 'CONTAINER', [makeNode(2, 'BUTTON')]);
    sm.loadSnapshot(makeGraph(10, root));

    sm.applyDiff(makeDiff(11, 10));
    expect(sm.getState().seq).toBe(11);
    expect(sm.getState().root).toEqual(root);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// applyDiff — UPDATE
// ─────────────────────────────────────────────────────────────────────────────

describe('StateManager — applyDiff (UPDATE)', () => {
  it('patches a label on a direct child node', () => {
    const sm = new StateManager();
    const btn = makeNode(2, 'BUTTON');
    const root = makeNode(1, 'CONTAINER', [btn]);
    sm.loadSnapshot(makeGraph(1, root));

    sm.applyDiff(makeDiff(2, 1, [
      { op: 'UPDATE', id: 2, patch: { label: 'Clicked!' } },
    ]));

    const updated = sm.getState().root.children?.[0];
    expect(updated?.label).toBe('Clicked!');
    expect(updated?.type).toBe('BUTTON'); // unchanged
  });

  it('patches multiple fields in one mutation', () => {
    const sm = new StateManager();
    const input: SceneNode = { id: 3, type: 'INPUT_FIELD', bounds: [0, 0, 200, 40], value: '' };
    sm.loadSnapshot(makeGraph(1, makeNode(1, 'CONTAINER', [input])));

    sm.applyDiff(makeDiff(2, 1, [
      { op: 'UPDATE', id: 3, patch: { value: 'hello@test.com', focused: true } },
    ]));

    const updated = sm.getState().root.children?.[0];
    expect(updated?.value).toBe('hello@test.com');
    expect(updated?.focused).toBe(true);
  });

  it('does not mutate previous state (immutability)', () => {
    const sm = new StateManager();
    const btn = makeNode(2, 'BUTTON');
    const root = makeNode(1, 'CONTAINER', [btn]);
    sm.loadSnapshot(makeGraph(1, root));
    const before = sm.getState().root;

    sm.applyDiff(makeDiff(2, 1, [
      { op: 'UPDATE', id: 2, patch: { label: 'New Label' } },
    ]));

    // The original root node reference should be unchanged
    expect(before.children?.[0]?.label).toBeUndefined();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// applyDiff — INSERT
// ─────────────────────────────────────────────────────────────────────────────

describe('StateManager — applyDiff (INSERT)', () => {
  it('inserts a new child node', () => {
    const sm = new StateManager();
    const root = makeNode(1, 'CONTAINER');
    sm.loadSnapshot(makeGraph(1, root));

    const newBtn = makeNode(99, 'BUTTON');
    sm.applyDiff(makeDiff(2, 1, [
      { op: 'INSERT', parent_id: 1, node: newBtn },
    ]));

    const children = sm.getState().root.children ?? [];
    expect(children).toHaveLength(1);
    expect(children[0]?.id).toBe(99);
    expect(children[0]?.type).toBe('BUTTON');
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// applyDiff — REMOVE
// ─────────────────────────────────────────────────────────────────────────────

describe('StateManager — applyDiff (REMOVE)', () => {
  it('removes a child node by id', () => {
    const sm = new StateManager();
    const btn = makeNode(2, 'BUTTON');
    const root = makeNode(1, 'CONTAINER', [btn]);
    sm.loadSnapshot(makeGraph(1, root));

    sm.applyDiff(makeDiff(2, 1, [
      { op: 'REMOVE', id: 2 },
    ]));

    const children = sm.getState().root.children ?? [];
    expect(children).toHaveLength(0);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// applyDiff — sequence validation
// ─────────────────────────────────────────────────────────────────────────────

describe('StateManager — sequence validation', () => {
  it('throws VactStateError for mismatched ack_seq', () => {
    const sm = new StateManager();
    sm.loadSnapshot(makeGraph(10, makeNode(1)));

    // ack_seq = 9 but current seq = 10 → missed frame
    expect(() => sm.applyDiff(makeDiff(11, 9))).toThrow(VactStateError);
  });

  it('throws VactStateError when no snapshot loaded', () => {
    const sm = new StateManager();
    expect(() => sm.applyDiff(makeDiff(2, 1))).toThrow(VactStateError);
  });
});
