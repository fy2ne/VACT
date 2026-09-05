//! Scene graph assembly logic — V8 core fusion algorithm.
//!
//! This module converts the raw perception outputs from V6 (containment hierarchy)
//! and V7 (OCR text regions) into the canonical VACT/1.0 [`SceneGraph`] AST.
//!
//! # Entry point
//!
//! [`build_scene_graph`] is the main function. Call it after every frame's
//! hierarchy and OCR extraction are complete.
//!
//! # Node classification
//!
//! Each [`vact_core::HierarchyNode`] is converted into a [`SceneNode`] by
//! consulting the OCR text region (if any) associated with the same `id`:
//!
//! | `SemanticRole`         | `NodeType`   | `label` | `value` | `interactable` |
//! |------------------------|--------------|---------|---------|----------------|
//! | `ButtonLabel`          | `Button`     | text    | –       | `true`         |
//! | `InputValue`           | `InputField` | text    | `""`    | `true`         |
//! | `Heading`              | `Text`       | text    | –       | –              |
//! | `BodyText`             | `Text`       | text    | –       | –              |
//! | `ListItem`             | `List`       | text    | –       | –              |
//! | `GeneralText`          | `Text`       | text    | –       | –              |
//! | (no text, has children)| `Container`  | –       | –       | –              |
//! | (no text, leaf, ≤48px) | `Icon`       | –       | –       | –              |
//! | (no text, leaf, large) | `Image`      | –       | –       | –              |

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use vact_core::{HierarchyNode, SemanticRole, TextRegion};

use crate::{NodeType, SceneGraph, SceneNode, Viewport};

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Assemble a [`SceneGraph`] from the V6 containment hierarchy and V7 text regions.
///
/// # Arguments
///
/// - `hierarchy`: root nodes from [`vact_core::build_hierarchy`] (V6).
/// - `text_regions`: OCR results from [`vactd::ocr::OcrEngine::extract_all`] (V7).
/// - `viewport_width` / `viewport_height`: captured frame dimensions in pixels.
/// - `seq`: monotonic frame counter used as the sequence number.
/// - `active_window`: title of the foreground window (from `GetForegroundWindow`).
///
/// # Virtual root wrapping
///
/// The VACT/1.0 spec requires a single `root` node. When the containment
/// hierarchy has exactly one root, it is promoted directly. When there are
/// zero or multiple roots, a synthetic `CONTAINER` with `id = 0` spanning
/// the full viewport is inserted. Real detected boxes always have `id ≥ 1`
/// (enforced by the CCL pipeline in V5).
pub fn build_scene_graph(
    hierarchy: &[HierarchyNode],
    text_regions: &[TextRegion],
    viewport_width: u32,
    viewport_height: u32,
    seq: u64,
    active_window: Option<String>,
) -> SceneGraph {
    // Build O(1) lookup: box id → TextRegion
    let text_map: HashMap<u32, &TextRegion> =
        text_regions.iter().map(|tr| (tr.id, tr)).collect();

    let viewport = Viewport {
        width: viewport_width,
        height: viewport_height,
        scale_factor: 1.0,
    };

    let timestamp_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);

    // Convert the hierarchy into SceneNodes
    let root_nodes: Vec<SceneNode> = hierarchy
        .iter()
        .map(|node| convert_node(node, &text_map))
        .collect();

    // Wrap or promote as appropriate
    let root = match root_nodes.len() {
        1 => root_nodes.into_iter().next().unwrap(),
        _ => {
            // 0 or 2+ roots → virtual container id=0 spanning the full viewport
            let mut virtual_root =
                SceneNode::container(0, [0, 0, viewport_width, viewport_height]);
            virtual_root.children = root_nodes;
            virtual_root
        }
    };

    SceneGraph::new(seq, timestamp_us, viewport, active_window, root)
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal conversion
// ─────────────────────────────────────────────────────────────────────────────

/// Recursively convert a [`HierarchyNode`] (and its children) into a [`SceneNode`].
fn convert_node(node: &HierarchyNode, text_map: &HashMap<u32, &TextRegion>) -> SceneNode {
    let bbox = &node.bounds;
    // Wire spec bounds: [x1, y1, x2, y2] (left, top, right, bottom)
    let bounds = [bbox.x, bbox.y, bbox.right(), bbox.bottom()];

    // Recurse into children first (bottom-up assembly)
    let children: Vec<SceneNode> = node
        .children
        .iter()
        .map(|child| convert_node(child, text_map))
        .collect();

    let has_children = !children.is_empty();

    // Classify this node
    let scene_node = if let Some(tr) = text_map.get(&node.id) {
        classify_text_node(node.id, bounds, tr, children)
    } else {
        classify_untextured_node(node.id, bounds, bbox, has_children, children)
    };

    scene_node
}

/// Classify a node that has an associated OCR [`TextRegion`].
fn classify_text_node(
    id: u32,
    bounds: [u32; 4],
    tr: &TextRegion,
    children: Vec<SceneNode>,
) -> SceneNode {
    let text = tr.text.trim().to_string();

    match tr.role {
        SemanticRole::ButtonLabel => SceneNode {
            id,
            node_type: NodeType::Button,
            bounds,
            label: Some(text),
            value: None,
            focused: None,
            interactable: Some(true),
            disabled: None,
            children,
            source: Some("ocr".to_string()),
            class_name: None,
            phash: None,
            color: None,
            color_hex: None,
        },
        SemanticRole::InputValue => SceneNode {
            id,
            node_type: NodeType::InputField,
            bounds,
            // The OCR text is the current value / placeholder
            label: Some(text.clone()),
            value: Some(text),
            focused: None,
            interactable: Some(true),
            disabled: None,
            children,
            source: Some("ocr".to_string()),
            class_name: None,
            phash: None,
            color: None,
            color_hex: None,
        },
        SemanticRole::ListItem => SceneNode {
            id,
            node_type: NodeType::List,
            bounds,
            label: Some(text),
            value: None,
            focused: None,
            interactable: None,
            disabled: None,
            children,
            source: Some("ocr".to_string()),
            class_name: None,
            phash: None,
            color: None,
            color_hex: None,
        },
        // Heading | BodyText | GeneralText → Text node
        SemanticRole::Heading | SemanticRole::BodyText | SemanticRole::GeneralText => SceneNode {
            id,
            node_type: NodeType::Text,
            bounds,
            label: Some(text),
            value: None,
            focused: None,
            interactable: None,
            disabled: None,
            children,
            source: Some("ocr".to_string()),
            class_name: None,
            phash: None,
            color: None,
            color_hex: None,
        },
    }
}

/// Classify a node that has **no** associated OCR text region.
///
/// Decision tree:
/// - Has children → `Container`
/// - Leaf, width/height ≤ 48 px, aspect ratio ∈ [0.5, 2.0] → `Icon`
/// - Leaf, otherwise → `Image`
fn classify_untextured_node(
    id: u32,
    bounds: [u32; 4],
    bbox: &vact_core::BoundingBox,
    has_children: bool,
    children: Vec<SceneNode>,
) -> SceneNode {
    let node_type = if has_children {
        NodeType::Container
    } else if is_icon_sized(bbox) {
        NodeType::Icon
    } else {
        NodeType::Image
    };

    SceneNode {
        id,
        node_type,
        bounds,
        label: None,
        value: None,
        focused: None,
        interactable: None,
        disabled: None,
        children,
        source: None,
        class_name: None,
        phash: None,
        color: None,
        color_hex: None,
    }
}

/// Traverses the [`SceneGraph`] and enriches every UI node with photometric
/// color metadata (`color` and `color_hex`) computed from the raw frame buffer.
pub fn enrich_scene_graph_colors(
    graph: &mut SceneGraph,
    bgra_data: &[u8],
    img_w: u32,
    img_h: u32,
) {
    if bgra_data.is_empty() || img_w == 0 || img_h == 0 {
        return;
    }
    enrich_node_color(&mut graph.root, bgra_data, img_w, img_h);
}

fn enrich_node_color(
    node: &mut SceneNode,
    bgra_data: &[u8],
    img_w: u32,
    img_h: u32,
) {
    // Only compute color for non-root detected nodes
    if node.id > 0 {
        let info = vact_core::color::analyze_region_color(
            bgra_data,
            img_w,
            img_h,
            node.bounds[0],
            node.bounds[1],
            node.bounds[2],
            node.bounds[3],
        );
        node.color = Some(info.name);
        node.color_hex = Some(info.hex);
    }

    for child in &mut node.children {
        enrich_node_color(child, bgra_data, img_w, img_h);
    }
}

/// Returns `true` when a bounding box looks like a small icon glyph:
/// - Both width and height are ≤ 48 px, AND
/// - Aspect ratio is between 0.5 and 2.0 (roughly square).
#[inline]
fn is_icon_sized(bbox: &vact_core::BoundingBox) -> bool {
    bbox.width <= 48 && bbox.height <= 48 && {
        let ar = bbox.aspect_ratio();
        ar >= 0.5 && ar <= 2.0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Debug printer
// ─────────────────────────────────────────────────────────────────────────────

/// Print the scene graph tree to `stdout` in an indented, human-readable format.
///
/// Shows node type, id, bounds, label, and interactability — useful for
/// verifying DAG structure in `--once` mode before the JSON dump.
///
/// # Example output
///
/// ```text
/// [CONTAINER] id=0   bounds=[0, 0, 1920, 1080]
/// ├─ [CONTAINER] id=1   bounds=[0, 0, 1920, 40]
/// │  ├─ [BUTTON]    id=3   bounds=[8, 4, 80, 36]    label="File"  ✓interactive
/// │  └─ [TEXT]      id=4   bounds=[90, 4, 200, 36]  label="Edit"
/// └─ [INPUT_FIELD] id=14  bounds=[340, 980, 1240, 1030]  label="Search"  ✓interactive
/// ```
pub fn print_scene_graph(graph: &SceneGraph) {
    println!(
        "── V8 Scene Graph  seq={}  active_window={:?}  viewport={}×{} ──",
        graph.seq,
        graph.active_window,
        graph.viewport.width,
        graph.viewport.height,
    );
    print_node(&graph.root, "", true);
}

fn print_node(node: &SceneNode, prefix: &str, is_last: bool) {
    let connector = if prefix.is_empty() {
        String::new()
    } else if is_last {
        format!("{}└─ ", prefix)
    } else {
        format!("{}├─ ", prefix)
    };

    let type_str = format!("{:?}", node.node_type);
    let label_str = match &node.label {
        Some(l) => format!("  label={:?}", l),
        None => String::new(),
    };
    let value_str = match &node.value {
        Some(v) => format!("  value={:?}", v),
        None => String::new(),
    };
    let interactive_str = if node.interactable == Some(true) {
        "  ✓interactive"
    } else {
        ""
    };

    println!(
        "{connector}[{type_str:<12}] id={id:<4}  bounds=[{x1}, {y1}, {x2}, {y2}]{label}{value}{interactive}",
        connector = connector,
        type_str = type_str,
        id = node.id,
        x1 = node.bounds[0],
        y1 = node.bounds[1],
        x2 = node.bounds[2],
        y2 = node.bounds[3],
        label = label_str,
        value = value_str,
        interactive = interactive_str,
    );

    let child_prefix = if prefix.is_empty() {
        String::new()
    } else if is_last {
        format!("{}   ", prefix)
    } else {
        format!("{}│  ", prefix)
    };

    let n = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        print_node(child, &child_prefix, i + 1 == n);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use vact_core::{BoundingBox, HierarchyNode, OcrWord, SemanticRole, TextRegion};

    // Helper: build a minimal HierarchyNode with no children
    fn leaf_node(id: u32, x: u32, y: u32, w: u32, h: u32) -> HierarchyNode {
        HierarchyNode {
            id,
            bounds: BoundingBox::new(id, x, y, w, h),
            depth: 0,
            children: vec![],
        }
    }

    // Helper: build a parent HierarchyNode with given children
    fn parent_node(id: u32, x: u32, y: u32, w: u32, h: u32, children: Vec<HierarchyNode>) -> HierarchyNode {
        HierarchyNode {
            id,
            bounds: BoundingBox::new(id, x, y, w, h),
            depth: 0,
            children,
        }
    }

    // Helper: build a TextRegion with the given role (bypasses infer_role)
    fn text_region_with_role(id: u32, x: u32, y: u32, w: u32, h: u32, text: &str, role: SemanticRole) -> TextRegion {
        let bbox = BoundingBox::new(id, x, y, w, h);
        // Build the TextRegion directly (role pre-set, bypassing heuristics)
        TextRegion {
            id: bbox.id,
            source_box: bbox,
            text: text.to_string(),
            words: vec![OcrWord::new(text, BoundingBox::new(0, x, y, w, h))],
            role,
        }
    }

    // ── Test 1: empty hierarchy ───────────────────────────────────────────────

    #[test]
    fn test_empty_hierarchy() {
        let sg = build_scene_graph(&[], &[], 1920, 1080, 1, None);
        // Should produce a virtual root CONTAINER id=0
        assert_eq!(sg.root.id, 0);
        assert_eq!(sg.root.node_type, NodeType::Container);
        assert_eq!(sg.root.bounds, [0, 0, 1920, 1080]);
        assert!(sg.root.children.is_empty());
        assert_eq!(sg.seq, 1);
        assert_eq!(sg.viewport.width, 1920);
        assert_eq!(sg.viewport.height, 1080);
        assert_eq!(sg.viewport.scale_factor, 1.0);
        assert_eq!(sg.protocol, "VACT/1.0");
    }

    // ── Test 2: single root, no text → Container ─────────────────────────────

    #[test]
    fn test_single_root_no_text() {
        // A single large leaf with no text is promoted directly (no virtual wrapping).
        // No children + not icon-sized → Image.
        let nodes = vec![leaf_node(1, 0, 0, 1920, 1080)];
        let sg = build_scene_graph(&nodes, &[], 1920, 1080, 2, None);
        // Single root: promoted directly (id=1)
        assert_eq!(sg.root.id, 1);
        // No children, size=1920×1080 → not icon-sized → Image
        assert_eq!(sg.root.node_type, NodeType::Image);
        assert!(sg.root.children.is_empty());
    }

    // ── Test 3: button classification ─────────────────────────────────────────

    #[test]
    fn test_button_classification() {
        let nodes = vec![leaf_node(5, 100, 100, 80, 32)];
        let trs = vec![text_region_with_role(5, 100, 100, 80, 32, "Submit", SemanticRole::ButtonLabel)];
        let sg = build_scene_graph(&nodes, &trs, 1920, 1080, 3, None);

        assert_eq!(sg.root.id, 5);
        assert_eq!(sg.root.node_type, NodeType::Button);
        assert_eq!(sg.root.label, Some("Submit".to_string()));
        assert_eq!(sg.root.interactable, Some(true));
        assert_eq!(sg.root.value, None);
    }

    // ── Test 4: input field classification ───────────────────────────────────

    #[test]
    fn test_input_field_classification() {
        let nodes = vec![leaf_node(7, 200, 300, 300, 28)];
        let trs = vec![text_region_with_role(7, 200, 300, 300, 28, "user@example.com", SemanticRole::InputValue)];
        let sg = build_scene_graph(&nodes, &trs, 1920, 1080, 4, None);

        assert_eq!(sg.root.id, 7);
        assert_eq!(sg.root.node_type, NodeType::InputField);
        assert_eq!(sg.root.label, Some("user@example.com".to_string()));
        assert_eq!(sg.root.value, Some("user@example.com".to_string()));
        assert_eq!(sg.root.interactable, Some(true));
    }

    // ── Test 5: heading classification ────────────────────────────────────────

    #[test]
    fn test_heading_classification() {
        let nodes = vec![leaf_node(9, 50, 50, 400, 40)];
        let trs = vec![text_region_with_role(9, 50, 50, 400, 40, "Account Settings", SemanticRole::Heading)];
        let sg = build_scene_graph(&nodes, &trs, 1920, 1080, 5, None);

        assert_eq!(sg.root.id, 9);
        assert_eq!(sg.root.node_type, NodeType::Text);
        assert_eq!(sg.root.label, Some("Account Settings".to_string()));
        assert_eq!(sg.root.interactable, None);
    }

    // ── Test 6: list item classification ──────────────────────────────────────

    #[test]
    fn test_list_item_classification() {
        let nodes = vec![leaf_node(11, 0, 100, 300, 24)];
        let trs = vec![text_region_with_role(11, 0, 100, 300, 24, "Inbox", SemanticRole::ListItem)];
        let sg = build_scene_graph(&nodes, &trs, 1920, 1080, 6, None);

        assert_eq!(sg.root.id, 11);
        assert_eq!(sg.root.node_type, NodeType::List);
        assert_eq!(sg.root.label, Some("Inbox".to_string()));
    }

    // ── Test 7: icon vs image heuristic ──────────────────────────────────────

    #[test]
    fn test_icon_vs_image_heuristic() {
        // Small square leaf → Icon
        let icon_nodes = vec![leaf_node(20, 10, 10, 24, 24)];
        let sg_icon = build_scene_graph(&icon_nodes, &[], 1920, 1080, 7, None);
        assert_eq!(sg_icon.root.id, 20);
        assert_eq!(sg_icon.root.node_type, NodeType::Icon);

        // Large leaf → Image
        let img_nodes = vec![leaf_node(21, 0, 0, 400, 300)];
        let sg_img = build_scene_graph(&img_nodes, &[], 1920, 1080, 8, None);
        assert_eq!(sg_img.root.id, 21);
        assert_eq!(sg_img.root.node_type, NodeType::Image);

        // Wide icon (aspect ratio > 2.0) → Image even if small height
        let wide_nodes = vec![leaf_node(22, 0, 0, 100, 20)];
        let sg_wide = build_scene_graph(&wide_nodes, &[], 1920, 1080, 9, None);
        assert_eq!(sg_wide.root.id, 22);
        // width=100 > 48 → Image regardless
        assert_eq!(sg_wide.root.node_type, NodeType::Image);
    }

    // ── Test 8: multi-root wrapping ───────────────────────────────────────────

    #[test]
    fn test_multi_root_wrapping() {
        let nodes = vec![
            leaf_node(1, 0, 0, 400, 300),
            leaf_node(2, 500, 0, 400, 300),
        ];
        let sg = build_scene_graph(&nodes, &[], 1920, 1080, 10, None);
        // 2 roots → virtual container id=0
        assert_eq!(sg.root.id, 0);
        assert_eq!(sg.root.node_type, NodeType::Container);
        assert_eq!(sg.root.bounds, [0, 0, 1920, 1080]);
        assert_eq!(sg.root.children.len(), 2);
        let ids: Vec<u32> = sg.root.children.iter().map(|n| n.id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
    }

    // ── Test 9: bounds conversion [x,y,w,h] → [x1,y1,x2,y2] ─────────────────

    #[test]
    fn test_bounds_conversion() {
        // BoundingBox(id=1, x=100, y=200, w=300, h=150)
        // Expected wire bounds: [100, 200, 400, 350]
        let nodes = vec![leaf_node(1, 100, 200, 300, 150)];
        let sg = build_scene_graph(&nodes, &[], 1920, 1080, 11, None);
        assert_eq!(sg.root.bounds, [100, 200, 400, 350]);
    }

    // ── Test 10: JSON wire format compliance ──────────────────────────────────

    #[test]
    fn test_json_wire_format() {
        let nodes = vec![
            parent_node(1, 0, 0, 1920, 1080, vec![
                leaf_node(2, 100, 100, 80, 32),
            ]),
        ];
        let trs = vec![
            text_region_with_role(2, 100, 100, 80, 32, "Send", SemanticRole::ButtonLabel),
        ];
        let sg = build_scene_graph(&nodes, &trs, 1920, 1080, 42, Some("Discord".to_string()));
        let json = serde_json::to_string(&sg).expect("serialization failed");

        // Field name checks
        assert!(json.contains("\"protocol\""), "missing protocol field");
        assert!(json.contains("\"VACT/1.0\""), "wrong protocol value");
        assert!(json.contains("\"seq\""), "missing seq field");
        assert!(json.contains("\"viewport\""), "missing viewport field");
        assert!(json.contains("\"scale_factor\""), "missing scale_factor");
        assert!(json.contains("\"active_window\""), "missing active_window");
        assert!(json.contains("\"Discord\""), "wrong active_window value");
        assert!(json.contains("\"root\""), "missing root field");
        assert!(json.contains("\"type\""), "missing type field");
        // NodeType serialization check
        assert!(json.contains("\"CONTAINER\""), "Container not SCREAMING_SNAKE_CASE");
        assert!(json.contains("\"BUTTON\""), "Button not SCREAMING_SNAKE_CASE");
        // Optional field omission: no focused/disabled on button
        assert!(!json.contains("\"focused\""), "focused should be omitted");
        assert!(!json.contains("\"disabled\""), "disabled should be omitted");
        // interactable IS present on button
        assert!(json.contains("\"interactable\""), "interactable missing from Button");
        // children omitted when empty on leaf
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let button = &parsed["root"]["children"][0];
        assert!(button["children"].is_null(), "empty children should be omitted");
    }

    // ── Test 11: optional field omission ──────────────────────────────────────

    #[test]
    fn test_optional_field_omission() {
        // A plain container (no text, has children) should not emit label/value/focused/disabled/interactable
        let nodes = vec![
            parent_node(1, 0, 0, 800, 600, vec![
                leaf_node(2, 10, 10, 20, 20), // will become Icon
            ]),
        ];
        let sg = build_scene_graph(&nodes, &[], 1920, 1080, 1, None);
        let json = serde_json::to_string_pretty(&sg).unwrap();

        // active_window is None → should not appear
        assert!(!json.contains("\"active_window\""), "active_window should be omitted when None");
        // Container root should have no label/value/focused/disabled/interactable
        assert!(!json.contains("\"label\""), "label should be absent");
        assert!(!json.contains("\"value\""), "value should be absent");
        assert!(!json.contains("\"focused\""), "focused should be absent");
        assert!(!json.contains("\"interactable\""), "interactable should be absent");
        assert!(!json.contains("\"disabled\""), "disabled should be absent");
    }

    // ── Test 12: nested hierarchy fusion ──────────────────────────────────────

    #[test]
    fn test_nested_hierarchy_fusion() {
        // Window → Toolbar (with Button) + Body (with InputField)
        let toolbar_btn = leaf_node(3, 10, 5, 60, 30);
        let body_input  = leaf_node(4, 50, 80, 300, 28);
        let toolbar     = parent_node(2, 0, 0, 1920, 40, vec![toolbar_btn]);
        let body        = parent_node(5, 0, 40, 1920, 1040, vec![body_input]);
        let window      = parent_node(1, 0, 0, 1920, 1080, vec![toolbar, body]);

        let trs = vec![
            text_region_with_role(3, 10, 5, 60, 30, "File",          SemanticRole::ButtonLabel),
            text_region_with_role(4, 50, 80, 300, 28, "Type here...", SemanticRole::InputValue),
        ];

        let sg = build_scene_graph(&[window], &trs, 1920, 1080, 1, None);

        // Single root → promoted directly (id=1, Container)
        assert_eq!(sg.root.id, 1);
        assert_eq!(sg.root.node_type, NodeType::Container);
        assert_eq!(sg.root.children.len(), 2);

        // Find toolbar child
        let toolbar_node = sg.root.children.iter().find(|n| n.id == 2).unwrap();
        assert_eq!(toolbar_node.node_type, NodeType::Container);
        assert_eq!(toolbar_node.children.len(), 1);

        let btn_node = &toolbar_node.children[0];
        assert_eq!(btn_node.id, 3);
        assert_eq!(btn_node.node_type, NodeType::Button);
        assert_eq!(btn_node.label, Some("File".to_string()));
        assert_eq!(btn_node.interactable, Some(true));

        // Find body child
        let body_node = sg.root.children.iter().find(|n| n.id == 5).unwrap();
        assert_eq!(body_node.node_type, NodeType::Container);

        let input_node = &body_node.children[0];
        assert_eq!(input_node.id, 4);
        assert_eq!(input_node.node_type, NodeType::InputField);
        assert_eq!(input_node.interactable, Some(true));
    }
}
