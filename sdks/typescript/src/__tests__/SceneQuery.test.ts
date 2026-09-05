/**
 * Tests for SceneQuery — fluent scene graph query DSL.
 */

import { describe, it, expect } from 'vitest';
import { SceneQuery } from '../query/SceneQuery.js';
import type { SceneNode } from '../types/scene.js';

// ─────────────────────────────────────────────────────────────────────────────
// Test tree
// ─────────────────────────────────────────────────────────────────────────────
//
//  root (CONTAINER, id=1)
//  ├── toolbar (CONTAINER, id=2)
//  │   ├── fileBtn  (BUTTON,      id=3,  label="File", interactable=true)
//  │   └── editBtn  (BUTTON,      id=4,  label="Edit", interactable=true, disabled=true)
//  └── body (CONTAINER, id=5)
//      ├── heading  (TEXT,        id=6,  label="Dashboard")
//      ├── search   (INPUT_FIELD, id=7,  label="Search", value="", interactable=true, focused=true)
//      └── sendBtn  (BUTTON,      id=8,  label="Send",  interactable=true)

function makeTree(): SceneNode {
  return {
    id: 1, type: 'CONTAINER', bounds: [0, 0, 1920, 1080],
    children: [
      {
        id: 2, type: 'CONTAINER', bounds: [0, 0, 1920, 40],
        children: [
          { id: 3, type: 'BUTTON', bounds: [0, 0, 80, 40], label: 'File', interactable: true },
          { id: 4, type: 'BUTTON', bounds: [90, 0, 170, 40], label: 'Edit', interactable: true, disabled: true },
        ],
      },
      {
        id: 5, type: 'CONTAINER', bounds: [0, 40, 1920, 1080],
        children: [
          { id: 6, type: 'TEXT',        bounds: [10, 50, 400, 90],   label: 'Dashboard' },
          { id: 7, type: 'INPUT_FIELD', bounds: [10, 100, 300, 130], label: 'Search', value: '', interactable: true, focused: true },
          { id: 8, type: 'BUTTON',      bounds: [1800, 1040, 1900, 1070], label: 'Send', interactable: true },
        ],
      },
    ],
  };
}

function query(root = makeTree()): SceneQuery {
  return new SceneQuery(root);
}

// ─────────────────────────────────────────────────────────────────────────────
// ofType
// ─────────────────────────────────────────────────────────────────────────────

describe('SceneQuery.ofType', () => {
  it('finds all BUTTON nodes', () => {
    const btns = query().ofType('BUTTON').all();
    expect(btns.map(n => n.id).sort()).toEqual([3, 4, 8]);
  });

  it('finds all CONTAINER nodes', () => {
    const containers = query().ofType('CONTAINER').all();
    expect(containers.map(n => n.id).sort()).toEqual([1, 2, 5]);
  });

  it('finds the INPUT_FIELD', () => {
    const inputs = query().ofType('INPUT_FIELD').all();
    expect(inputs).toHaveLength(1);
    expect(inputs[0]?.id).toBe(7);
  });

  it('returns empty for absent type', () => {
    expect(query().ofType('ICON').all()).toHaveLength(0);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// withLabel
// ─────────────────────────────────────────────────────────────────────────────

describe('SceneQuery.withLabel', () => {
  it('finds a node by exact label string', () => {
    const node = query().withLabel('Send').first();
    expect(node?.id).toBe(8);
  });

  it('finds nodes by regex', () => {
    const nodes = query().withLabel(/^(File|Edit)$/).all();
    expect(nodes.map(n => n.id).sort()).toEqual([3, 4]);
  });

  it('is case-sensitive by default for strings', () => {
    expect(query().withLabel('send').first()).toBeUndefined();
  });

  it('case-insensitive with regex /i flag', () => {
    expect(query().withLabel(/send/i).first()?.id).toBe(8);
  });

  it('returns undefined when no match', () => {
    expect(query().withLabel('NonExistent').first()).toBeUndefined();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// byId
// ─────────────────────────────────────────────────────────────────────────────

describe('SceneQuery.byId', () => {
  it('finds a node by exact id', () => {
    expect(query().byId(6).first()?.label).toBe('Dashboard');
  });

  it('returns undefined for missing id', () => {
    expect(query().byId(999).first()).toBeUndefined();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// interactable / notDisabled / disabled / focused
// ─────────────────────────────────────────────────────────────────────────────

describe('SceneQuery boolean filters', () => {
  it('.interactable() returns only interactable nodes', () => {
    const nodes = query().interactable().all();
    expect(nodes.every(n => n.interactable === true)).toBe(true);
    expect(nodes.map(n => n.id).sort()).toEqual([3, 4, 7, 8]);
  });

  it('.focused() returns only focused nodes', () => {
    const nodes = query().focused().all();
    expect(nodes).toHaveLength(1);
    expect(nodes[0]?.id).toBe(7);
  });

  it('.notDisabled() excludes disabled nodes', () => {
    const nodes = query().ofType('BUTTON').notDisabled().all();
    expect(nodes.map(n => n.id)).not.toContain(4);
  });

  it('.disabled() returns only disabled nodes', () => {
    const nodes = query().disabled().all();
    expect(nodes).toHaveLength(1);
    expect(nodes[0]?.id).toBe(4);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// containingPoint
// ─────────────────────────────────────────────────────────────────────────────

describe('SceneQuery.containingPoint', () => {
  it('finds nodes containing a screen point', () => {
    // Point (40, 20) is inside the toolbar (0-1920, 0-40) and fileBtn (0-80, 0-40)
    const nodes = query().containingPoint(40, 20).all();
    const ids = nodes.map(n => n.id).sort();
    expect(ids).toContain(1); // root
    expect(ids).toContain(2); // toolbar
    expect(ids).toContain(3); // fileBtn
  });

  it('returns empty when point is outside all nodes', () => {
    const nodes = query().containingPoint(9999, 9999).all();
    expect(nodes).toHaveLength(0);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Chaining / composition
// ─────────────────────────────────────────────────────────────────────────────

describe('SceneQuery — chaining', () => {
  it('chains ofType + withLabel', () => {
    const node = query().ofType('BUTTON').withLabel(/^send$/i).first();
    expect(node?.id).toBe(8);
  });

  it('chains ofType + interactable + notDisabled', () => {
    const nodes = query().ofType('BUTTON').interactable().notDisabled().all();
    expect(nodes.map(n => n.id).sort()).toEqual([3, 8]);
  });

  it('.where() applies custom predicate', () => {
    // Find buttons with label length <= 4
    const nodes = query().ofType('BUTTON').where(n => (n.label?.length ?? 0) <= 4).all();
    expect(nodes.map(n => n.label?.toLowerCase()).sort()).toEqual(['edit', 'file', 'send']);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// Terminal operators
// ─────────────────────────────────────────────────────────────────────────────

describe('SceneQuery terminals', () => {
  it('.count() returns correct count', () => {
    expect(query().ofType('BUTTON').count()).toBe(3);
  });

  it('.exists() returns true when match found', () => {
    expect(query().ofType('BUTTON').exists()).toBe(true);
  });

  it('.exists() returns false when no match', () => {
    expect(query().ofType('ICON').exists()).toBe(false);
  });

  it('.all() returns depth-first order', () => {
    // All nodes DFS from root
    const all = query().all();
    expect(all[0]?.id).toBe(1); // root first
    expect(all[1]?.id).toBe(2); // toolbar before body
  });
});
