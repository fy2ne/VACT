/**
 * SceneQuery — fluent query DSL for the VACT scene graph.
 *
 * Provides a jQuery-inspired API for finding nodes in the current scene graph
 * without manual tree traversal.
 *
 * ```typescript
 * // Find the first interactable button labeled "Send"
 * const sendBtn = client.query()
 *   .ofType('BUTTON')
 *   .withLabel(/send/i)
 *   .interactable()
 *   .first();
 *
 * // Find all visible input fields
 * const inputs = client.query()
 *   .ofType('INPUT_FIELD')
 *   .notDisabled()
 *   .all();
 *
 * // Find by stable node ID
 * const node = client.query().byId(42).first();
 * ```
 */

import type { SceneNode, NodeType } from '../types/scene.js';

// ─────────────────────────────────────────────────────────────────────────────
// SceneQuery
// ─────────────────────────────────────────────────────────────────────────────

type NodePredicate = (node: SceneNode) => boolean;

/**
 * Fluent, composable scene graph query builder.
 *
 * Each filter method returns a new `SceneQuery` instance (immutable chaining).
 * Use {@link first}, {@link all}, or {@link count} to execute the query.
 */
export class SceneQuery {
  private readonly _root: SceneNode;
  private readonly _predicates: readonly NodePredicate[];

  /** @internal */
  public constructor(root: SceneNode, predicates: readonly NodePredicate[] = []) {
    this._root = root;
    this._predicates = predicates;
  }

  // ── Filters ───────────────────────────────────────────────────────────────

  /**
   * Filter to nodes of the given semantic type.
   *
   * @example `.ofType('BUTTON')`
   */
  public ofType(type: NodeType): SceneQuery {
    return this._add((n) => n.type === type);
  }

  /**
   * Filter to nodes whose `label` matches a string or regex.
   *
   * @example `.withLabel('Send')` or `.withLabel(/send/i)`
   */
  public withLabel(pattern: string | RegExp): SceneQuery {
    return this._add((n) => {
      if (n.label === undefined) return false;
      if (typeof pattern === 'string') return n.label === pattern;
      return pattern.test(n.label);
    });
  }

  /**
   * Filter to nodes whose `value` matches a string or regex.
   *
   * @example `.withValue('')` (empty input fields)
   */
  public withValue(pattern: string | RegExp): SceneQuery {
    return this._add((n) => {
      if (n.value === undefined) return false;
      if (typeof pattern === 'string') return n.value === pattern;
      return pattern.test(n.value);
    });
  }

  /**
   * Filter to a node by its stable ID.
   *
   * @example `.byId(14)`
   */
  public byId(id: number): SceneQuery {
    return this._add((n) => n.id === id);
  }

  /**
   * Filter to nodes that have `interactable: true`.
   */
  public interactable(): SceneQuery {
    return this._add((n) => n.interactable === true);
  }

  /**
   * Filter to nodes that have `focused: true`.
   */
  public focused(): SceneQuery {
    return this._add((n) => n.focused === true);
  }

  /**
   * Filter to nodes that do **not** have `disabled: true`.
   */
  public notDisabled(): SceneQuery {
    return this._add((n) => n.disabled !== true);
  }

  /**
   * Filter to nodes that have `disabled: true`.
   */
  public disabled(): SceneQuery {
    return this._add((n) => n.disabled === true);
  }

  /**
   * Filter to nodes whose bounds contain the given screen point.
   *
   * @param x - X coordinate in screen pixels
   * @param y - Y coordinate in screen pixels
   */
  public containingPoint(x: number, y: number): SceneQuery {
    return this._add(
      (n) =>
        n.bounds[0] <= x &&
        x <= n.bounds[2] &&
        n.bounds[1] <= y &&
        y <= n.bounds[3],
    );
  }

  /**
   * Apply a custom predicate filter.
   *
   * @example `.where(n => (n.bounds[2] - n.bounds[0]) > 100)`
   */
  public where(predicate: (node: SceneNode) => boolean): SceneQuery {
    return this._add(predicate);
  }

  // ── Terminal operations ───────────────────────────────────────────────────

  /**
   * Execute the query and return the first matching node.
   * Returns `undefined` if no match is found.
   */
  public first(): SceneNode | undefined {
    return this._collect(true)[0];
  }

  /**
   * Execute the query and return all matching nodes (depth-first order).
   */
  public all(): SceneNode[] {
    return this._collect(false);
  }

  /**
   * Execute the query and return the number of matching nodes.
   */
  public count(): number {
    return this._collect(false).length;
  }

  /**
   * Execute the query and return `true` if at least one node matches.
   */
  public exists(): boolean {
    return this._collect(true).length > 0;
  }

  // ── Private ───────────────────────────────────────────────────────────────

  private _add(pred: NodePredicate): SceneQuery {
    return new SceneQuery(this._root, [...this._predicates, pred]);
  }

  private _collect(stopAtFirst: boolean): SceneNode[] {
    const results: SceneNode[] = [];
    this._walk(this._root, results, stopAtFirst);
    return results;
  }

  /** Depth-first tree traversal. */
  private _walk(
    node: SceneNode,
    results: SceneNode[],
    stopAtFirst: boolean,
  ): boolean /* found */ {
    if (this._matches(node)) {
      results.push(node);
      if (stopAtFirst) return true;
    }
    for (const child of node.children ?? []) {
      if (this._walk(child, results, stopAtFirst) && stopAtFirst) return true;
    }
    return false;
  }

  private _matches(node: SceneNode): boolean {
    return this._predicates.every((pred) => pred(node));
  }
}
