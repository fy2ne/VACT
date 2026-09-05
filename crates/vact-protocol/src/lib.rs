//! `vact-protocol` — VACT/1.0 wire format, scene graph AST, and IPC framing.
//!
//! This crate owns the canonical schema for everything that travels over the
//! named-pipe / shared-memory transport between `vactd` and AI agent clients.
//!
//! # V8 additions
//!
//! [`NodeType`], [`SceneNode`], [`Viewport`], and [`SceneGraph`] define the
//! full VACT/1.0 AST snapshot message — the Vector Scene Graph (DAG) that
//! fuses spatial containment hierarchy (V6) with OCR text regions (V7) into
//! a typed, JSON-serializable scene representation.
//!
//! [`scene`] module contains the assembly logic:
//! - [`scene::build_scene_graph`]: fuses `HierarchyNode` tree + `TextRegion` OCR data.
//! - [`scene::print_scene_graph`]: debug tree printer (similar to V6 `print_tree`).
//!
//! Future versions will add:
//! - `DiffFrame` temporal delta messages           (V9)
//! - MessagePack / JSON serialization helpers      (V10)
//! - Client handshake & capability negotiation     (V10)

use serde::{Deserialize, Serialize};

pub mod diff;
pub mod scene;
pub mod ipc;

// ─────────────────────────────────────────────────────────────────────────────
// Protocol constants
// ─────────────────────────────────────────────────────────────────────────────

/// The VACT wire protocol version string sent in every frame header.
pub const PROTOCOL_VERSION: &str = "VACT/1.0";

// ─────────────────────────────────────────────────────────────────────────────
// V8 — Scene Graph AST Types
// ─────────────────────────────────────────────────────────────────────────────

/// Semantic node type for a UI element in the VACT scene graph.
///
/// Serialized as SCREAMING_SNAKE_CASE strings in JSON (e.g. `"BUTTON"`,
/// `"INPUT_FIELD"`) to match the VACT/1.0 wire specification.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NodeType {
    /// A generic UI container / panel with no specific interactive role.
    Container,
    /// An interactive button element (e.g. "Save", "Cancel", "Submit").
    Button,
    /// An editable text input, search field, or form control.
    InputField,
    /// A static text label, heading, body text, or list item.
    Text,
    /// A non-interactive image, illustration, or background graphic.
    Image,
    /// A scrollable list, menu, or table container.
    List,
    /// A small icon or glyph (typically ≤ 48×48 px, roughly square).
    Icon,
}

/// A single node in the VACT Vector Scene Graph (DAG).
///
/// Nodes form a recursive directed acyclic graph rooted at a single top-level
/// `CONTAINER` node. Properties follow the VACT/1.0 wire specification.
///
/// # Bounds format
///
/// `bounds` is stored as `[x1, y1, x2, y2]` (left, top, right, bottom) in
/// absolute screen pixels — **not** `(x, y, width, height)`. This matches
/// the wire spec and differs from the internal [`vact_core::BoundingBox`]
/// representation. Conversion: `[bbox.x, bbox.y, bbox.right(), bbox.bottom()]`.
///
/// # Optional fields
///
/// Fields that are `None` are omitted from JSON serialization to keep the
/// delta stream compact (e.g. `focused`, `interactable`, `disabled` are only
/// emitted when meaningful).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneNode {
    /// Unique identifier within the current frame, matching the source
    /// [`vact_core::BoundingBox::id`]. Virtual root uses `id = 0`.
    pub id: u32,
    /// Semantic type of this UI element.
    #[serde(rename = "type")]
    pub node_type: NodeType,
    /// Bounding rectangle as `[x1, y1, x2, y2]` in absolute screen pixels.
    pub bounds: [u32; 4],
    /// Text label — button caption, heading, static text, or list item value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Current value — populated for `INPUT_FIELD` nodes (empty string if
    /// the field is empty and OCR saw a placeholder).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Whether this element currently has keyboard focus.
    /// `None` when focus state is unknown (V8); set in V11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    /// Whether this element accepts user interaction (click / type).
    /// Set to `true` for `BUTTON` and `INPUT_FIELD` nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactable: Option<bool>,
    /// Whether this element is visually or logically disabled.
    /// `None` when disabled state is unknown (V8); set in V11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Direct child nodes. Omitted from JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<SceneNode>,
    /// Source provenance of the node metadata ("ocr", "uiautomation", "win32", "memory", "taskbar").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Window class or UIAutomation control class name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// Perceptual hash (dHash) hex string for visual caching and agent memory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phash: Option<String>,
    /// Semantic photometric color name ("red", "blue", "green", "orange", "yellow", "purple", "cyan", "dark_gray", "white", "black").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Primary 6-character hex code in `#RRGGBB` format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_hex: Option<String>,
}

impl SceneNode {
    /// Create a minimal container node (used for the virtual root and plain containers).
    pub fn container(id: u32, bounds: [u32; 4]) -> Self {
        Self {
            id,
            node_type: NodeType::Container,
            bounds,
            label: None,
            value: None,
            focused: None,
            interactable: None,
            disabled: None,
            children: Vec::new(),
            source: None,
            class_name: None,
            phash: None,
            color: None,
            color_hex: None,
        }
    }
}

/// Viewport metadata describing the captured display surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    /// Width of the captured desktop surface in physical pixels.
    pub width: u32,
    /// Height of the captured desktop surface in physical pixels.
    pub height: u32,
    /// Display DPI scale factor relative to 96 DPI.
    /// Hardcoded to `1.0` in V8; real monitor DPI awareness added in V10.
    pub scale_factor: f32,
}

/// Full-frame VACT/1.0 scene graph snapshot message.
///
/// This is the top-level JSON document emitted by `vactd` over the IPC
/// transport. It contains the complete Vector Scene Graph for the current
/// frame, identified by a monotonic sequence number.
///
/// # Wire schema example
///
/// ```json
/// {
///   "protocol": "VACT/1.0",
///   "seq": 1042,
///   "timestamp_us": 1729384920194,
///   "viewport": { "width": 1920, "height": 1080, "scale_factor": 1.0 },
///   "active_window": "Discord",
///   "root": {
///     "id": 1,
///     "type": "CONTAINER",
///     "bounds": [0, 0, 1920, 1080],
///     "children": [ ... ]
///   }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneGraph {
    /// Wire protocol version. Always `"VACT/1.0"`.
    pub protocol: String,
    /// Monotonically increasing frame sequence number.
    pub seq: u64,
    /// Frame capture timestamp in microseconds since Unix epoch (or daemon start).
    pub timestamp_us: u64,
    /// Dimensions and scale of the captured display surface.
    pub viewport: Viewport,
    /// Title of the foreground window at capture time. `None` when unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_window: Option<String>,
    /// Root of the scene graph DAG.
    pub root: SceneNode,
}

impl SceneGraph {
    /// Create a new `SceneGraph` with the given root node and frame metadata.
    ///
    /// Sets `protocol` to [`PROTOCOL_VERSION`] automatically.
    pub fn new(
        seq: u64,
        timestamp_us: u64,
        viewport: Viewport,
        active_window: Option<String>,
        root: SceneNode,
    ) -> Self {
        Self {
            protocol: PROTOCOL_VERSION.to_string(),
            seq,
            timestamp_us,
            viewport,
            active_window,
            root,
        }
    }
}

