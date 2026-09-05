//! V11 — Hybrid I/O Bus integration tests
//!
//! These tests verify:
//!   1. Route-mode detection logic (class-name heuristic)
//!   2. Coordinate resolution from a mock scene graph
//!   3. AgentAction serialization matches the VACT/1.0 IPC wire format
//!   4. IoBus dispatch returns a coherent ActionResult for known/unknown nodes
//!   5. IpcMessage Action round-trips through JSON correctly

use vact_protocol::{SceneNode, SceneGraph, Viewport, NodeType};
use vact_protocol::ipc::IpcMessage;
use vactd::io_bus::{AgentAction, IoBus, RouteMode, resolve_coords};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn make_graph() -> SceneGraph {
    let mut root = SceneNode::container(0, [0, 0, 1920, 1080]);

    let mut button = SceneNode::container(42, [100, 200, 300, 240]);
    button.node_type = NodeType::Button;
    button.label = Some("Send".into());
    button.interactable = Some(true);

    let mut input = SceneNode::container(99, [400, 900, 1200, 950]);
    input.node_type = NodeType::InputField;
    input.value = Some(String::new());
    input.interactable = Some(true);

    root.children.push(button);
    root.children.push(input);

    SceneGraph::new(
        1,
        999_000,
        Viewport { width: 1920, height: 1080, scale_factor: 1.0 },
        Some("TestApp".into()),
        root,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — Coordinate resolution from scene graph
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_resolve_coords_hit() {
    let graph = make_graph();
    // Node 42 bounds = [100, 200, 300, 240] → centre = (200, 220)
    let coords = resolve_coords(&graph, 42);
    assert_eq!(coords, Some((200, 220)));
}

#[test]
fn test_resolve_coords_miss() {
    let graph = make_graph();
    // Node 9999 does not exist
    let coords = resolve_coords(&graph, 9999);
    assert_eq!(coords, None);
}

#[test]
fn test_resolve_coords_zero() {
    let graph = make_graph();
    // target_id = 0 must always return None (direct/focused typing)
    let coords = resolve_coords(&graph, 0);
    assert_eq!(coords, None);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — IoBus dispatch: missing node returns error result
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_dispatch_click_missing_node() {
    let bus = IoBus::new();
    bus.update_scene(make_graph());

    let result = bus.dispatch(AgentAction::Click { target_id: 9999 });
    assert!(!result.ok, "Dispatch on missing node should fail");
    assert!(result.error.is_some());
    assert!(result.error.unwrap().contains("9999"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — IoBus dispatch: no scene graph loaded returns error
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_dispatch_no_scene() {
    let bus = IoBus::new();
    // Do NOT call bus.update_scene()
    let result = bus.dispatch(AgentAction::Click { target_id: 42 });
    assert!(!result.ok);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — AgentAction JSON serialization (wire format compliance)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_agent_action_json_click() {
    let action = AgentAction::Click { target_id: 42 };
    let json = serde_json::to_string(&action).unwrap();
    assert!(json.contains("\"CLICK\""), "action tag missing: {}", json);
    assert!(json.contains("42"), "target_id missing: {}", json);
}

#[test]
fn test_agent_action_json_type() {
    let action = AgentAction::Type { target_id: 99, text: "hello".into() };
    let json = serde_json::to_string(&action).unwrap();
    assert!(json.contains("\"TYPE\""), "action tag missing: {}", json);
    assert!(json.contains("hello"), "text missing: {}", json);
}

#[test]
fn test_agent_action_json_scroll() {
    let action = AgentAction::Scroll { target_id: 1, delta_y: -3 };
    let json = serde_json::to_string(&action).unwrap();
    assert!(json.contains("\"SCROLL\""), "action tag missing: {}", json);
    assert!(json.contains("-3"), "delta_y missing: {}", json);
}

#[test]
fn test_agent_action_json_key() {
    let action = AgentAction::Key { target_id: 1, vk: 0x0D }; // VK_RETURN
    let json = serde_json::to_string(&action).unwrap();
    assert!(json.contains("\"KEY\""), "action tag missing: {}", json);
    assert!(json.contains("13"), "vk missing: {}", json);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — IpcMessage Action round-trip through JSON
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ipc_action_roundtrip() {
    let msg = IpcMessage::Action {
        action: "CLICK".into(),
        target_id: 42,
        text: None,
        delta_y: None,
        vk: None,
        label: None,
        action_result: None,
    };

    let json = serde_json::to_string(&msg).unwrap();
    let decoded: IpcMessage = serde_json::from_str(&json).unwrap();

    match decoded {
        IpcMessage::Action { action, target_id, text, delta_y, vk, label, action_result } => {
            assert_eq!(action, "CLICK");
            assert_eq!(target_id, 42);
            assert!(text.is_none());
            assert!(delta_y.is_none());
            assert!(vk.is_none());
            assert!(label.is_none());
            assert!(action_result.is_none());
        }
        _ => panic!("Expected IpcMessage::Action, got something else"),
    }
}

#[test]
fn test_ipc_action_type_roundtrip() {
    let msg = IpcMessage::Action {
        action: "TYPE".into(),
        target_id: 99,
        text: Some("hello world".into()),
        delta_y: None,
        vk: None,
        label: None,
        action_result: None,
    };

    let json = serde_json::to_string(&msg).unwrap();
    let decoded: IpcMessage = serde_json::from_str(&json).unwrap();

    match decoded {
        IpcMessage::Action { action, target_id, text, .. } => {
            assert_eq!(action, "TYPE");
            assert_eq!(target_id, 99);
            assert_eq!(text.unwrap(), "hello world");
        }
        _ => panic!("Expected IpcMessage::Action"),
    }
}

#[test]
fn test_ipc_action_result_roundtrip() {
    let msg = IpcMessage::ActionResult {
        action: "CLICK".into(),
        target_id: 42,
        route: "Direct".into(),
        ok: true,
        error: None,
    };

    let json = serde_json::to_string(&msg).unwrap();
    let decoded: IpcMessage = serde_json::from_str(&json).unwrap();

    match decoded {
        IpcMessage::ActionResult { action, target_id, ok, error, .. } => {
            assert_eq!(action, "CLICK");
            assert_eq!(target_id, 42);
            assert!(ok);
            assert!(error.is_none());
        }
        _ => panic!("Expected IpcMessage::ActionResult"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — RouteMode serialization
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_route_mode_serialization() {
    let direct = serde_json::to_string(&RouteMode::Direct).unwrap();
    let ghost = serde_json::to_string(&RouteMode::Ghost).unwrap();
    assert_eq!(direct, "\"DIRECT\"");
    assert_eq!(ghost, "\"GHOST\"");
}

#[test]
fn test_to_absolute_viewport_normalization() {
    use vactd::io_bus::to_absolute;

    // Origin (0, 0) should map to (0, 0)
    let (nx, ny) = to_absolute(0, 0, 1920, 1080);
    assert_eq!((nx, ny), (0, 0));

    // Viewport max (1920, 1080) maps to 65535
    let (nx, ny) = to_absolute(1920, 1080, 1920, 1080);
    assert_eq!((nx, ny), (65535, 65535));

    // Center (960, 540) maps to ~32767
    let (nx, ny) = to_absolute(960, 540, 1920, 1080);
    assert!((nx as i32 - 32767).abs() <= 1);
    assert!((ny as i32 - 32767).abs() <= 1);
}

#[test]
fn test_learn_action_dispatch() {
    let memory = std::sync::Arc::new(vactd::memory::MemoryStore::open_in_memory().unwrap());
    let bus = IoBus::with_memory(std::sync::Arc::clone(&memory));

    let node = SceneNode {
        id: 77,
        node_type: NodeType::Icon,
        bounds: [100, 100, 150, 150],
        label: None,
        value: None,
        focused: None,
        interactable: None,
        disabled: None,
        children: Vec::new(),
        source: None,
        class_name: None,
        phash: Some("1122334455667788".to_string()),
        color: None,
        color_hex: None,
    };
    let graph = SceneGraph {
        protocol: "VACT/1.0".to_string(),
        seq: 1,
        timestamp_us: 1000,
        viewport: Viewport { width: 1920, height: 1080, scale_factor: 1.0 },
        active_window: Some("test_app.exe".to_string()),
        root: node,
    };
    bus.update_scene(graph);

    let res = bus.dispatch(AgentAction::Learn {
        target_id: 77,
        label: "Settings Gear".to_string(),
        action_result: Some("opened_settings".to_string()),
    });

    assert!(res.ok);
    assert_eq!(res.action, "LEARN");

    // Verify persisted in memory store
    let entry = memory.lookup("1122334455667788", "test_app.exe").expect("lookup failed");
    assert_eq!(entry.label, "Settings Gear");
    assert_eq!(entry.action_result.as_deref(), Some("opened_settings"));
}
