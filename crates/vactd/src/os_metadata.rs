//! VACT OS Metadata Fusion Layer
//!
//! Fuses OS-level accessibility trees (UIAutomation) and Win32 window metadata
//! with visual computer vision detections to enrich unlabelled or ambiguous UI
//! elements (e.g. icon buttons, input fields, custom controls).

use vact_protocol::{NodeType, SceneGraph, SceneNode};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationElement};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowTextW, WindowFromPoint};

/// OS metadata provider encapsulating Win32 and UIAutomation COM clients.
pub struct OsMetadataProvider {
    uia: Option<IUIAutomation>,
}

impl OsMetadataProvider {
    /// Create a new `OsMetadataProvider`, initializing COM and the UIAutomation client.
    pub fn new() -> Self {
        // Initialize COM on this thread (safe to call multiple times)
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        let uia: Option<IUIAutomation> = unsafe {
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
        };

        Self { uia }
    }

    /// Query OS metadata for a specific screen coordinate (x, y).
    pub fn inspect_point(&self, x: i32, y: i32) -> Option<OsElementInfo> {
        let pt = POINT { x, y };

        // 1. Try UIAutomation first (richest accessibility metadata)
        if let Some(ref uia) = self.uia {
            if let Ok(elem) = unsafe { uia.ElementFromPoint(pt) } {
                if let Some(info) = extract_uia_info(&elem) {
                    if !info.is_empty() {
                        return Some(info);
                    }
                }
            }
        }

        // 2. Fall back to Win32 Window inspection
        inspect_win32_point(pt)
    }

    /// Enrich all nodes in a SceneGraph with OS metadata where visual/OCR
    /// labels are absent or ambiguous.
    pub fn enrich_scene_graph(&self, graph: &mut SceneGraph) {
        enrich_node_recursive(&mut graph.root, self);
    }
}

impl Default for OsMetadataProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Extracted OS metadata for a UI element.
#[derive(Clone, Debug, Default)]
pub struct OsElementInfo {
    pub name: Option<String>,
    pub control_type: Option<NodeType>,
    pub class_name: Option<String>,
    pub automation_id: Option<String>,
    pub source: &'static str,
    pub interactable: Option<bool>,
}

impl OsElementInfo {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.control_type.is_none() && self.class_name.is_none()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// UIAutomation Extraction
// ─────────────────────────────────────────────────────────────────────────────

fn extract_uia_info(elem: &IUIAutomationElement) -> Option<OsElementInfo> {
    let name = unsafe { elem.CurrentName().ok().map(|b| b.to_string()).filter(|s| !s.trim().is_empty()) };
    let control_type_id = unsafe { elem.CurrentControlType().ok().map(|id| id.0).unwrap_or(0) };
    let class_name = unsafe { elem.CurrentClassName().ok().map(|b| b.to_string()).filter(|s| !s.trim().is_empty()) };
    let automation_id = unsafe { elem.CurrentAutomationId().ok().map(|b| b.to_string()).filter(|s| !s.trim().is_empty()) };

    let (node_type, interactable) = map_uia_control_type(control_type_id);

    Some(OsElementInfo {
        name,
        control_type: node_type,
        class_name,
        automation_id,
        source: "uiautomation",
        interactable,
    })
}

/// Map UIA ControlTypeId to VACT [`NodeType`] and default interactable flag.
fn map_uia_control_type(type_id: i32) -> (Option<NodeType>, Option<bool>) {
    // Standard UIA ControlType IDs
    match type_id {
        50000 => (Some(NodeType::Button), Some(true)),       // UIA_ButtonControlTypeId
        50002 => (Some(NodeType::Button), Some(true)),       // UIA_CheckBoxControlTypeId
        50003 => (Some(NodeType::List), Some(true)),         // UIA_ComboBoxControlTypeId
        50004 => (Some(NodeType::InputField), Some(true)),   // UIA_EditControlTypeId
        50005 => (Some(NodeType::Button), Some(true)),       // UIA_HyperlinkControlTypeId
        50006 => (Some(NodeType::Image), None),              // UIA_ImageControlTypeId
        50007 | 50008 => (Some(NodeType::List), None),       // UIA_ListItem / ListControlTypeId
        50009 | 50010 | 50011 => (Some(NodeType::Button), Some(true)), // Menu / MenuItem
        50013 => (Some(NodeType::Button), Some(true)),       // UIA_RadioButtonControlTypeId
        50015 | 50016 => (Some(NodeType::InputField), Some(true)), // Slider / Spinner
        50018 | 50019 => (Some(NodeType::Button), Some(true)), // Tab / TabItem
        50020 => (Some(NodeType::Text), None),               // UIA_TextControlTypeId
        50023 | 50024 => (Some(NodeType::List), None),       // Tree / TreeItem
        50031 => (Some(NodeType::Button), Some(true)),       // UIA_SplitButtonControlTypeId
        50034 | 50035 => (Some(NodeType::Text), None),       // Header / HeaderItem
        _ => (None, None),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Win32 Extraction
// ─────────────────────────────────────────────────────────────────────────────

fn inspect_win32_point(pt: POINT) -> Option<OsElementInfo> {
    let hwnd = unsafe { WindowFromPoint(pt) };
    if hwnd.0.is_null() {
        return None;
    }

    let class_name = get_win32_class(hwnd);
    let window_text = get_win32_text(hwnd);

    let (node_type, interactable) = if let Some(ref cls) = class_name {
        map_win32_class(cls)
    } else {
        (None, None)
    };

    Some(OsElementInfo {
        name: window_text,
        control_type: node_type,
        class_name,
        automation_id: None,
        source: "win32",
        interactable,
    })
}

fn get_win32_class(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len > 0 {
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    } else {
        None
    }
}

fn get_win32_text(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len > 0 {
        let s = String::from_utf16_lossy(&buf[..len as usize]);
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn map_win32_class(class_name: &str) -> (Option<NodeType>, Option<bool>) {
    let lower = class_name.to_lowercase();
    if lower.contains("button") {
        (Some(NodeType::Button), Some(true))
    } else if lower.contains("edit") || lower.contains("richedit") {
        (Some(NodeType::InputField), Some(true))
    } else if lower.contains("static") {
        (Some(NodeType::Text), None)
    } else if lower.contains("listview") || lower.contains("treeview") || lower.contains("listbox") || lower.contains("combobox") {
        (Some(NodeType::List), None)
    } else {
        (None, None)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Recursive SceneGraph Enrichment
// ─────────────────────────────────────────────────────────────────────────────

fn enrich_node_recursive(node: &mut SceneNode, provider: &OsMetadataProvider) {
    // Center point of node bounding rect
    let [x1, y1, x2, y2] = node.bounds;
    let cx = ((x1 + x2) / 2) as i32;
    let cy = ((y1 + y2) / 2) as i32;

    // Only query OS metadata if the node lacks OCR label, or is an icon/image/generic container
    let is_candidate = node.label.is_none()
        || node.node_type == NodeType::Icon
        || node.node_type == NodeType::Image
        || (node.node_type == NodeType::Container && node.children.is_empty());

    if is_candidate {
        if let Some(info) = provider.inspect_point(cx, cy) {
            if node.label.is_none() {
                if let Some(ref name) = info.name {
                    node.label = Some(name.clone());
                }
            }
            if let Some(control_type) = info.control_type {
                if node.node_type == NodeType::Icon || node.node_type == NodeType::Image || (node.node_type == NodeType::Container && node.children.is_empty()) {
                    node.node_type = control_type;
                }
            }
            if node.class_name.is_none() {
                node.class_name = info.class_name;
            }
            if node.source.is_none() && (node.label.is_some() || node.class_name.is_some()) {
                node.source = Some(info.source.to_string());
            }
            if node.interactable.is_none() {
                node.interactable = info.interactable;
            }
        }
    }

    for child in &mut node.children {
        enrich_node_recursive(child, provider);
    }
}
