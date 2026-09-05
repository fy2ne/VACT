/**
 * VACT/1.0 Scene Graph Types
 *
 * Mirrors the canonical Rust wire schema from `vact-protocol::scene`.
 * All field names and value formats are spec-compliant.
 *
 * @see {@link https://vact.fy2ne.me/spec/scene-graph}
 */

// ─────────────────────────────────────────────────────────────────────────────
// NodeType
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Semantic type of a UI element in the VACT scene graph.
 *
 * Serialized as SCREAMING_SNAKE_CASE strings on the wire (e.g. `"BUTTON"`,
 * `"INPUT_FIELD"`) — matches the VACT/1.0 specification exactly.
 */
export type NodeType =
  | 'CONTAINER'
  | 'BUTTON'
  | 'INPUT_FIELD'
  | 'TEXT'
  | 'IMAGE'
  | 'LIST'
  | 'ICON';

// ─────────────────────────────────────────────────────────────────────────────
// Viewport
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Viewport metadata describing the captured display surface.
 */
export interface Viewport {
  /** Width of the captured desktop surface in physical pixels. */
  readonly width: number;
  /** Height of the captured desktop surface in physical pixels. */
  readonly height: number;
  /**
   * Display DPI scale factor relative to 96 DPI.
   * e.g. `1.25` on a 125% Windows scaling display.
   */
  readonly scale_factor: number;
}

// ─────────────────────────────────────────────────────────────────────────────
// Bounds
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Bounding rectangle in the VACT/1.0 wire format: `[x1, y1, x2, y2]`
 * (left, top, right, bottom) in absolute screen pixels.
 *
 * Note: this differs from the `(x, y, width, height)` convention.
 * Use {@link boundsToRect} for conversion.
 */
export type Bounds = readonly [number, number, number, number];

/**
 * Convert a wire `Bounds` tuple to a `{x, y, width, height}` rect.
 */
export function boundsToRect(bounds: Bounds): {
  x: number;
  y: number;
  width: number;
  height: number;
} {
  return {
    x: bounds[0],
    y: bounds[1],
    width: bounds[2] - bounds[0],
    height: bounds[3] - bounds[1],
  };
}

/**
 * Compute the center point of a `Bounds` tuple.
 */
export function boundsCenter(bounds: Bounds): { x: number; y: number } {
  return {
    x: Math.round((bounds[0] + bounds[2]) / 2),
    y: Math.round((bounds[1] + bounds[3]) / 2),
  };
}

// ─────────────────────────────────────────────────────────────────────────────
// SceneNode
// ─────────────────────────────────────────────────────────────────────────────

/**
 * A single node in the VACT Vector Scene Graph (DAG).
 *
 * Nodes form a recursive directed acyclic graph rooted at a single top-level
 * `CONTAINER` node. All properties follow the VACT/1.0 wire specification.
 *
 * Optional fields (`label`, `value`, `focused`, `interactable`, `disabled`,
 * `children`) are omitted from JSON serialization when absent to keep the
 * delta stream compact.
 */
export interface SceneNode {
  /**
   * Stable identifier for this node, assigned by the `vactd` state tracker.
   * Unique within the current session. Virtual root uses `id = 0`.
   */
  readonly id: number;

  /** Semantic type of this UI element. */
  readonly type: NodeType;

  /**
   * Bounding rectangle: `[x1, y1, x2, y2]` in absolute screen pixels.
   * Use {@link boundsToRect} or {@link boundsCenter} for coordinate helpers.
   */
  readonly bounds: Bounds;

  /**
   * Text label — button caption, heading, static text, or list item value.
   * `undefined` when no OCR text is associated with this node.
   */
  readonly label?: string;

  /**
   * Current value — populated for `INPUT_FIELD` nodes.
   * Empty string when the field is empty.
   * `undefined` for non-input nodes.
   */
  readonly value?: string;

  /**
   * Whether this element currently has keyboard focus.
   * `undefined` when focus state is not tracked by the daemon.
   */
  readonly focused?: boolean;

  /**
   * Whether this element accepts user interaction (click / type / scroll).
   * `true` for `BUTTON` and `INPUT_FIELD` nodes.
   * `undefined` when interactability is not determined.
   */
  readonly interactable?: boolean;

  /**
   * Whether this element is visually or logically disabled.
   * `undefined` when disabled state is not tracked.
   */
  readonly disabled?: boolean;

  /**
   * Metadata source for this node (e.g., `"ocr"`, `"taskbar"`, `"uiautomation"`, `"win32"`, `"memory"`).
   */
  readonly source?: string;

  /**
   * Native OS control class name (e.g. `"Button"`, `"Edit"`, `"SysListView32"`).
   */
  readonly class_name?: string;

  /**
   * 64-bit gradient difference perceptual hash (16-char hex).
   */
  readonly phash?: string;

  /**
   * Semantic photometric color name (e.g. "red", "blue", "green", "orange", "yellow", "purple", "cyan", "dark_gray", "white", "black").
   */
  readonly color?: string;

  /**
   * Primary 6-character hex code in `#RRGGBB` format.
   */
  readonly color_hex?: string;

  /** Direct child nodes. Empty array when this is a leaf node. */
  readonly children?: readonly SceneNode[];
}

// ─────────────────────────────────────────────────────────────────────────────
// SceneGraph
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Full-frame VACT/1.0 scene graph snapshot.
 *
 * This is the top-level document emitted by `vactd` on the initial connection
 * and whenever a full state reset is required. Subsequent updates arrive as
 * {@link DiffFrame} temporal deltas.
 *
 * @example
 * ```json
 * {
 *   "protocol": "VACT/1.0",
 *   "seq": 1042,
 *   "timestamp_us": 1729384920194,
 *   "viewport": { "width": 1920, "height": 1080, "scale_factor": 1.25 },
 *   "active_window": "Discord",
 *   "root": { "id": 1, "type": "CONTAINER", "bounds": [0, 0, 1920, 1080], "children": [] }
 * }
 * ```
 */
export interface SceneGraph {
  /** Wire protocol version. Always `"VACT/1.0"`. */
  readonly protocol: string;

  /** Monotonically increasing frame sequence number. */
  readonly seq: number;

  /**
   * Frame capture timestamp in microseconds since Unix epoch.
   * Use `BigInt(timestamp_us)` if you need full 64-bit precision.
   */
  readonly timestamp_us: number;

  /** Dimensions and DPI scale of the captured display. */
  readonly viewport: Viewport;

  /**
   * Title of the foreground window at capture time.
   * `undefined` when unavailable (minimized, locked screen, etc.).
   */
  readonly active_window?: string;

  /** Root of the scene graph DAG. */
  readonly root: SceneNode;
}
