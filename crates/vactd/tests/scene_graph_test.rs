//! Integration test for V8 — Vector Scene Graph (DAG) Construction.
//!
//! Constructs a realistic mock perception pipeline output (hierarchy + OCR text
//! regions) and verifies the full end-to-end scene graph assembly, JSON
//! serialization, and round-trip deserialization.

use vact_core::{BoundingBox, HierarchyNode, OcrWord, SemanticRole, TextRegion};
use vact_protocol::{NodeType, SceneGraph, SceneNode};
use vact_protocol::scene::build_scene_graph;

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

fn make_node(id: u32, x: u32, y: u32, w: u32, h: u32, children: Vec<HierarchyNode>) -> HierarchyNode {
    HierarchyNode {
        id,
        bounds: BoundingBox::new(id, x, y, w, h),
        depth: 0,
        children,
    }
}

fn make_text_region(id: u32, x: u32, y: u32, w: u32, h: u32, text: &str, role: SemanticRole) -> TextRegion {
    let bbox = BoundingBox::new(id, x, y, w, h);
    TextRegion {
        id,
        source_box: bbox,
        text: text.to_string(),
        words: vec![OcrWord::new(text, bbox)],
        role,
    }
}

fn find_node<'a>(node: &'a SceneNode, id: u32) -> Option<&'a SceneNode> {
    if node.id == id {
        return Some(node);
    }
    node.children.iter().find_map(|c| find_node(c, id))
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration Test: full pipeline mock → SceneGraph → JSON round-trip
// ─────────────────────────────────────────────────────────────────────────────

/// Simulate a realistic Discord-like UI scene:
///
/// ```
/// Window (id=1, 1920×1080)
/// ├─ Sidebar (id=2, 0–240, 0–1080)  [Container]
/// │  ├─ Icon: Logo     (id=3,  8×8, 16×16)   → Icon
/// │  ├─ Text: #general (id=4,  8×50, 200×24) → List
/// │  └─ Text: #random  (id=5,  8×82, 200×24) → List
/// └─ Main (id=6, 240–1920, 0–1080)            [Container]
///    ├─ Header (id=7, 240×0, 1680×40)          → Text (Heading)
///    ├─ Messages (id=8, 240×40, 1680×940)      → Container (no text)
///    └─ Composer bar (id=9, 240×940, 1680×1080)
///       ├─ InputField (id=10, 260×950, 1300×1040) → InputField
///       └─ Send button (id=11, 1580×950, 1680×1040) → Button
/// ```
#[test]
fn test_full_scene_graph_pipeline() {
    // Build hierarchy
    let logo     = make_node(3, 8,   16,  16,  16,  vec![]);
    let general  = make_node(4, 8,   50,  200, 24,  vec![]);
    let random   = make_node(5, 8,   82,  200, 24,  vec![]);
    let sidebar  = make_node(2, 0,   0,   240, 1080, vec![logo, general, random]);

    let header   = make_node(7, 240, 0,   1680, 40,  vec![]);
    let messages = make_node(8, 240, 40,  1680, 940, vec![]);
    let input    = make_node(10, 260, 950, 1300, 90, vec![]);
    let send_btn = make_node(11, 1580, 950, 100, 90, vec![]);
    let composer = make_node(9, 240, 940, 1680, 140, vec![input, send_btn]);
    let main_pane = make_node(6, 240, 0,  1680, 1080, vec![header, messages, composer]);

    let window = make_node(1, 0, 0, 1920, 1080, vec![sidebar, main_pane]);
    let hierarchy = vec![window];

    // Build OCR text regions
    let trs = vec![
        make_text_region(4, 8,   50,  200, 24, "#general",      SemanticRole::ListItem),
        make_text_region(5, 8,   82,  200, 24, "#random",       SemanticRole::ListItem),
        make_text_region(7, 240, 0,  1680, 40, "# general",     SemanticRole::Heading),
        make_text_region(10, 260, 950, 1300, 90, "Message #general", SemanticRole::InputValue),
        make_text_region(11, 1580, 950, 100, 90, "Send",         SemanticRole::ButtonLabel),
    ];

    // Build scene graph
    let sg = build_scene_graph(
        &hierarchy,
        &trs,
        1920,
        1080,
        42,
        Some("Discord".to_string()),
    );

    // ── Top-level structure ────────────────────────────────────────────────
    assert_eq!(sg.protocol, "VACT/1.0");
    assert_eq!(sg.seq, 42);
    assert_eq!(sg.active_window, Some("Discord".to_string()));
    assert_eq!(sg.viewport.width, 1920);
    assert_eq!(sg.viewport.height, 1080);
    assert_eq!(sg.viewport.scale_factor, 1.0);

    // Single root → promoted directly (no virtual wrapper)
    assert_eq!(sg.root.id, 1);
    assert_eq!(sg.root.node_type, NodeType::Container);
    assert_eq!(sg.root.bounds, [0, 0, 1920, 1080]);
    assert_eq!(sg.root.children.len(), 2);

    // ── Sidebar ───────────────────────────────────────────────────────────
    let sidebar_node = find_node(&sg.root, 2).expect("sidebar node missing");
    assert_eq!(sidebar_node.node_type, NodeType::Container);

    // Logo: 16×16 square → Icon
    let logo_node = find_node(&sg.root, 3).expect("logo node missing");
    assert_eq!(logo_node.node_type, NodeType::Icon);
    assert_eq!(logo_node.label, None);

    // Channel names: ListItem → List
    let general_node = find_node(&sg.root, 4).expect("#general missing");
    assert_eq!(general_node.node_type, NodeType::List);
    assert_eq!(general_node.label, Some("#general".to_string()));
    assert_eq!(general_node.interactable, None);

    let random_node = find_node(&sg.root, 5).expect("#random missing");
    assert_eq!(random_node.node_type, NodeType::List);
    assert_eq!(random_node.label, Some("#random".to_string()));

    // ── Main pane ─────────────────────────────────────────────────────────
    let main_node = find_node(&sg.root, 6).expect("main pane missing");
    assert_eq!(main_node.node_type, NodeType::Container);

    // Header: Heading → Text
    let header_node = find_node(&sg.root, 7).expect("header missing");
    assert_eq!(header_node.node_type, NodeType::Text);
    assert_eq!(header_node.label, Some("# general".to_string()));

    // Messages: no text, has no children in this mock → Image (large leaf)
    let msg_node = find_node(&sg.root, 8).expect("messages pane missing");
    assert_eq!(msg_node.node_type, NodeType::Image);  // no text, no children, large → Image

    // Composer bar: no text, has children → Container
    let composer_node = find_node(&sg.root, 9).expect("composer missing");
    assert_eq!(composer_node.node_type, NodeType::Container);
    assert_eq!(composer_node.children.len(), 2);

    // Input field
    let input_node = find_node(&sg.root, 10).expect("input field missing");
    assert_eq!(input_node.node_type, NodeType::InputField);
    assert_eq!(input_node.label, Some("Message #general".to_string()));
    assert_eq!(input_node.value, Some("Message #general".to_string()));
    assert_eq!(input_node.interactable, Some(true));

    // Send button
    let send_node = find_node(&sg.root, 11).expect("send button missing");
    assert_eq!(send_node.node_type, NodeType::Button);
    assert_eq!(send_node.label, Some("Send".to_string()));
    assert_eq!(send_node.interactable, Some(true));
    assert_eq!(send_node.value, None);

    // ── JSON round-trip ───────────────────────────────────────────────────
    let json = serde_json::to_string_pretty(&sg).expect("serialization failed");

    // Verify the JSON is non-empty and valid
    assert!(!json.is_empty());
    let round_tripped: SceneGraph = serde_json::from_str(&json)
        .expect("JSON round-trip deserialization failed");

    // Round-tripped graph is structurally identical
    assert_eq!(round_tripped.protocol, sg.protocol);
    assert_eq!(round_tripped.seq, sg.seq);
    assert_eq!(round_tripped.active_window, sg.active_window);
    assert_eq!(round_tripped.viewport.width, sg.viewport.width);
    assert_eq!(round_tripped.viewport.height, sg.viewport.height);
    assert_eq!(round_tripped.root.id, sg.root.id);
    assert_eq!(round_tripped.root.node_type, sg.root.node_type);
    assert_eq!(round_tripped.root.children.len(), sg.root.children.len());

    // Verify key wire format details in raw JSON string
    assert!(json.contains("\"protocol\""));
    assert!(json.contains("\"VACT/1.0\""));
    assert!(json.contains("\"CONTAINER\""));
    assert!(json.contains("\"BUTTON\""));
    assert!(json.contains("\"INPUT_FIELD\""));
    assert!(json.contains("\"LIST\""));
    assert!(json.contains("\"TEXT\""));
    assert!(json.contains("\"ICON\""));
    assert!(json.contains("\"scale_factor\""));
    assert!(json.contains("\"interactable\""));
    // No focus/disabled emitted (all None)
    assert!(!json.contains("\"focused\""));
    assert!(!json.contains("\"disabled\""));
    // active_window present
    assert!(json.contains("\"Discord\""));
}

// ─────────────────────────────────────────────────────────────────────────────
// Edge case: empty hierarchy → virtual root id=0
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integration_empty_hierarchy_virtual_root() {
    let sg = build_scene_graph(&[], &[], 2560, 1440, 1, None);
    assert_eq!(sg.root.id, 0);
    assert_eq!(sg.root.node_type, NodeType::Container);
    assert_eq!(sg.root.bounds, [0, 0, 2560, 1440]);
    assert!(sg.root.children.is_empty());

    // JSON round-trip
    let json = serde_json::to_string(&sg).unwrap();
    let rt: SceneGraph = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.root.id, 0);
    // active_window absent from JSON when None
    assert!(!json.contains("active_window"));
}
