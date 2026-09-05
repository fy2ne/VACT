//! V11 — Hybrid I/O Bus (Direct + Ghost Mode)
//!
//! Routes agent actions to the target window using two strategies:
//!
//! - **Direct Mode** (`SendInput`): Injects hardware-level synthetic mouse /
//!   keyboard events through the OS hardware input queue. Works on all apps
//!   (Electron, Chromium, Figma, WebGL). Requires the target window to be in
//!   the foreground (or at least on the interactive desktop).
//!
//! - **Ghost Mode** (`PostMessageW`): Posts Win32 messages directly to the
//!   target HWND's message queue — no cursor movement, no window focus
//!   required. Works on standard Win32 / native controls only.
//!
//! **Auto-routing:** We query `GetWindowLongW(GWL_EXSTYLE)` and check the
//! class name. If the window is a plain `#32770` dialog or `BUTTON` / `EDIT`
//! native control we use Ghost mode; otherwise we fall back to Direct mode.

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetSystemMetrics, PostMessageW,
    SM_CXSCREEN, SM_CYSCREEN, WM_CHAR, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP,
};

// ─────────────────────────────────────────────────────────────────────────────
// Public Action Schema
// ─────────────────────────────────────────────────────────────────────────────

/// The routing mode chosen for an action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RouteMode {
    /// Hardware injection via `SendInput` (works on all window types).
    Direct,
    /// Background Win32 message posting via `PostMessageW` (native controls only).
    Ghost,
}

/// A single agent action targeting a UI element by scene-graph node ID.
///
/// Coordinates are resolved by the [`IoBus`] from the scene graph bounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentAction {
    /// Click the centre of the node's bounding box.
    Click { target_id: u32 },
    /// Type text into the focused element (or the given node).
    Type { target_id: u32, text: String },
    /// Scroll at the node's centre.
    Scroll { target_id: u32, delta_y: i32 },
    /// Send a key press by virtual-key code.
    Key { target_id: u32, vk: u16 },
    /// Move focus to the node (click-to-focus then click-away-less).
    Focus { target_id: u32 },
    /// Learn / label a UI element in the persistent agent memory cache.
    Learn {
        target_id: u32,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        action_result: Option<String>,
    },
}

/// Result of dispatching an action.
#[derive(Clone, Debug)]
pub struct ActionResult {
    pub action: String,
    pub route: RouteMode,
    pub target_id: u32,
    pub coords: Option<(i32, i32)>,
    pub ok: bool,
    pub error: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers — coordinate resolution
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the screen centre of a node from the last known scene graph.
pub fn resolve_coords(
    graph: &vact_protocol::SceneGraph,
    target_id: u32,
) -> Option<(i32, i32)> {
    if target_id == 0 {
        return None;
    }
    find_node(&graph.root, target_id).map(|n| {
        let cx = ((n.bounds[0] + n.bounds[2]) / 2) as i32;
        let cy = ((n.bounds[1] + n.bounds[3]) / 2) as i32;
        (cx, cy)
    })
}

fn find_node<'a>(
    node: &'a vact_protocol::SceneNode,
    id: u32,
) -> Option<&'a vact_protocol::SceneNode> {
    if node.id == id {
        return Some(node);
    }
    node.children.iter().find_map(|c| find_node(c, id))
}

// ─────────────────────────────────────────────────────────────────────────────
// Auto-routing — class name heuristic
// ─────────────────────────────────────────────────────────────────────────────

/// Native Win32 class names that support Ghost-mode PostMessage.
const GHOST_CLASSES: &[&str] = &[
    "#32770", // Dialog
    "Button",
    "Edit",
    "Static",
    "ListBox",
    "ComboBox",
    "ScrollBar",
    "msctls_trackbar32",
    "SysListView32",
    "SysTreeView32",
];

/// Determine routing mode for the foreground window.
pub fn detect_route_mode() -> RouteMode {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return RouteMode::Direct;
    }

    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return RouteMode::Direct;
    }

    let class = String::from_utf16_lossy(&buf[..len as usize]);
    if GHOST_CLASSES.iter().any(|&c| c.eq_ignore_ascii_case(&class)) {
        RouteMode::Ghost
    } else {
        RouteMode::Direct
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Direct Mode — SendInput helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a physical pixel (x, y) to the `SendInput` normalised 0–65535 range
/// using the exact captured viewport dimensions (falling back to GetSystemMetrics).
pub fn to_absolute(x: i32, y: i32, vw: i32, vh: i32) -> (i32, i32) {
    let sw = if vw > 0 { vw } else { unsafe { GetSystemMetrics(SM_CXSCREEN) } };
    let sh = if vh > 0 { vh } else { unsafe { GetSystemMetrics(SM_CYSCREEN) } };
    let nx = (x * 65535) / sw.max(1);
    let ny = (y * 65535) / sh.max(1);
    (nx, ny)
}

/// Smoothly glides the mouse cursor to (target_x, target_y) using human-like ease-in-out interpolation.
pub fn smooth_mouse_move(target_x: i32, target_y: i32, vw: i32, vh: i32) {
    let mut cur = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cur);
    }
    let start_x = cur.x as f32;
    let start_y = cur.y as f32;
    let end_x = target_x as f32;
    let end_y = target_y as f32;

    let dist = ((end_x - start_x).powi(2) + (end_y - start_y).powi(2)).sqrt();
    if dist < 4.0 {
        let (ax, ay) = to_absolute(target_x, target_y, vw, vh);
        let move_ev = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: ax,
                    dy: ay,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let _ = unsafe { SendInput(&[move_ev], std::mem::size_of::<INPUT>() as i32) };
        return;
    }

    // Number of micro-steps scaled by pixel distance (min 8, max 20 steps)
    let steps = ((dist / 45.0).clamp(8.0, 20.0)) as usize;
    let step_delay = std::time::Duration::from_millis((120 / steps).clamp(4, 12) as u64);

    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        // Smoothstep S-curve (human acceleration + deceleration)
        let ease = t * t * (3.0 - 2.0 * t);
        let curr_x = (start_x + (end_x - start_x) * ease).round() as i32;
        let curr_y = (start_y + (end_y - start_y) * ease).round() as i32;

        let (ax, ay) = to_absolute(curr_x, curr_y, vw, vh);
        let move_ev = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: ax,
                    dy: ay,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let _ = unsafe { SendInput(&[move_ev], std::mem::size_of::<INPUT>() as i32) };
        std::thread::sleep(step_delay);
    }
}

/// Move + left-click at absolute screen coordinates via `SendInput`.
pub fn direct_click(x: i32, y: i32, vw: i32, vh: i32) -> windows::core::Result<()> {
    // 1. Smoothly glide cursor across the screen to target coordinates
    smooth_mouse_move(x, y, vw, vh);

    let (ax, ay) = to_absolute(x, y, vw, vh);

    let down_ev = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax,
                dy: ay,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let up_ev = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax,
                dy: ay,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_LEFTUP | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let events = [down_ev, up_ev];
    let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
    if sent != events.len() as u32 {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

/// Type a UTF-16 string via `SendInput` KEYEVENTF_UNICODE key events.
pub fn direct_type(text: &str) -> windows::core::Result<()> {
    let mut events: Vec<INPUT> = Vec::with_capacity(text.encode_utf16().count() * 2);

    for ch in text.encode_utf16() {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        events.push(down);
        events.push(up);
    }

    if events.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
    if sent != events.len() as u32 {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

/// Send a virtual-key press via `SendInput`.
pub fn direct_key(vk: u16) -> windows::core::Result<()> {
    let vkey = VIRTUAL_KEY(vk);
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;

    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vkey,
                wScan: scan,
                dwFlags: KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vkey,
                wScan: scan,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let events = [down, up];
    let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
    if sent != events.len() as u32 {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

/// Send a vertical scroll wheel event at the given screen coordinates.
pub fn direct_scroll(x: i32, y: i32, delta_y: i32, vw: i32, vh: i32) -> windows::core::Result<()> {
    let (ax, ay) = to_absolute(x, y, vw, vh);
    let ev = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax,
                dy: ay,
                // WHEEL_DELTA = 120 per notch; positive = up, negative = down
                mouseData: (delta_y * 120) as u32,
                dwFlags: MOUSEEVENTF_WHEEL | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[ev], std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Ghost Mode — PostMessageW helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Post a left-click to the target HWND without moving the cursor.
///
/// `lx`, `ly` are client-area coordinates relative to the HWND.
pub fn ghost_click(hwnd: HWND, lx: i32, ly: i32) -> windows::core::Result<()> {
    let lparam = LPARAM((((ly as u32 as usize) << 16) | (lx as u32 as usize & 0xffff)) as isize);
    unsafe {
        PostMessageW(Some(hwnd), WM_LBUTTONDOWN, WPARAM(1), lparam)?;
        PostMessageW(Some(hwnd), WM_LBUTTONUP, WPARAM(0), lparam)?;
    }
    Ok(())
}

/// Post WM_CHAR messages for each character in `text` to `hwnd`.
pub fn ghost_type(hwnd: HWND, text: &str) -> windows::core::Result<()> {
    for ch in text.chars() {
        let w = WPARAM(ch as usize);
        unsafe {
            PostMessageW(Some(hwnd), WM_CHAR, w, LPARAM(1))?;
        }
    }
    Ok(())
}

/// Post WM_KEYDOWN + WM_KEYUP for the given virtual-key code to `hwnd`.
pub fn ghost_key(hwnd: HWND, vk: u16) -> windows::core::Result<()> {
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as usize;
    let repeat_lparam = LPARAM(((scan << 16) | 1) as isize);
    let up_lparam = LPARAM(((1usize << 31) | (1usize << 30) | (scan << 16) | 1) as isize);
    unsafe {
        PostMessageW(Some(hwnd), WM_KEYDOWN, WPARAM(vk as usize), repeat_lparam)?;
        PostMessageW(Some(hwnd), WM_KEYUP, WPARAM(vk as usize), up_lparam)?;
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// I/O Bus — public dispatch interface
// ─────────────────────────────────────────────────────────────────────────────

/// The Hybrid I/O Bus. Holds a reference to the latest scene graph so it can
/// resolve node IDs to pixel coordinates before dispatching actions.
pub struct IoBus {
    scene: std::sync::Arc<std::sync::Mutex<Option<vact_protocol::SceneGraph>>>,
    emergency_stopped: std::sync::atomic::AtomicBool,
    last_click: std::sync::Mutex<Option<(u32, std::time::Instant)>>,
    memory: Option<std::sync::Arc<crate::memory::MemoryStore>>,
}

impl IoBus {
    pub fn new() -> Self {
        Self {
            scene: std::sync::Arc::new(std::sync::Mutex::new(None)),
            emergency_stopped: std::sync::atomic::AtomicBool::new(false),
            last_click: std::sync::Mutex::new(None),
            memory: None,
        }
    }

    pub fn with_memory(memory: std::sync::Arc<crate::memory::MemoryStore>) -> Self {
        Self {
            scene: std::sync::Arc::new(std::sync::Mutex::new(None)),
            emergency_stopped: std::sync::atomic::AtomicBool::new(false),
            last_click: std::sync::Mutex::new(None),
            memory: Some(memory),
        }
    }

    /// Emergency stop: freeze all I/O dispatch immediately (triggered by F12 safety key).
    pub fn trigger_emergency_stop(&self) {
        self.emergency_stopped.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Reset emergency stop and restore normal I/O bus operation.
    pub fn reset_emergency_stop(&self) {
        self.emergency_stopped.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Query whether the safety emergency stop is currently active.
    pub fn is_emergency_stopped(&self) -> bool {
        self.emergency_stopped.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Update the cached scene graph (called after every V9 diff pass).
    pub fn update_scene(&self, graph: vact_protocol::SceneGraph) {
        *self.scene.lock().unwrap() = Some(graph);
    }

    /// Dispatch an agent action. Returns an [`ActionResult`] describing what
    /// happened — never panics.
    pub fn dispatch(&self, action: AgentAction) -> ActionResult {
        let (action_name, target_id) = match &action {
            AgentAction::Click { target_id } => ("CLICK", *target_id),
            AgentAction::Type { target_id, .. } => ("TYPE", *target_id),
            AgentAction::Scroll { target_id, .. } => ("SCROLL", *target_id),
            AgentAction::Key { target_id, .. } => ("KEY", *target_id),
            AgentAction::Focus { target_id } => ("FOCUS", *target_id),
            AgentAction::Learn { target_id, .. } => ("LEARN", *target_id),
        };

        // 🚨 Safety Check: If Emergency Stop is active, reject action immediately
        if self.is_emergency_stopped() {
            log::warn!("🚨 Action {} on node #{} BLOCKED — Global Emergency Stop (F12) is ACTIVE.", action_name, target_id);
            return ActionResult {
                action: action_name.to_string(),
                route: RouteMode::Direct,
                target_id,
                coords: None,
                ok: false,
                error: Some("Global Emergency Stop (F12) is active. Synthetic I/O is locked.".to_string()),
            };
        }

        // Handle LEARN action directly (updates memory store and local scene graph)
        if let AgentAction::Learn { target_id, ref label, ref action_result } = action {
            let mut scene_guard = self.scene.lock().unwrap();
            let mut learned = false;
            let mut phash_found = None;
            let mut app_name = "*".to_string();

            if let Some(ref mut scene) = *scene_guard {
                if let Some(ref active) = scene.active_window {
                    app_name = active.clone();
                }
                if let Some(node) = find_node_mut(&mut scene.root, target_id) {
                    node.label = Some(label.clone());
                    node.source = Some("memory".to_string());
                    if let Some(ref ph) = node.phash {
                        phash_found = Some(ph.clone());
                    }
                    learned = true;
                }
            }

            if let (Some(phash), Some(ref mem)) = (phash_found, &self.memory) {
                let _ = mem.learn(&phash, &app_name, label, "BUTTON", action_result.as_deref());
            }

            return ActionResult {
                action: "LEARN".to_string(),
                route: RouteMode::Direct,
                target_id,
                coords: None,
                ok: learned,
                error: if learned {
                    None
                } else {
                    Some(format!("Node #{} not found in active scene graph", target_id))
                },
            };
        }

        let scene_guard = self.scene.lock().unwrap();
        let scene_ref = scene_guard.as_ref();

        let coords = scene_ref.and_then(|g| resolve_coords(g, target_id));
        let (vw, vh) = scene_ref
            .map(|g| (g.viewport.width as i32, g.viewport.height as i32))
            .unwrap_or((0, 0));
        let route = detect_route_mode();
        let hwnd = unsafe { GetForegroundWindow() };

        if let Some((x, y)) = coords {
            crate::overlay::DebugOverlay::register_click(x as u32, y as u32, action_name, target_id);
        }

        let result = match action {
            AgentAction::Click { .. } => {
                if let Some((x, y)) = coords {
                    // Update last click record for launch debounce tracking
                    *self.last_click.lock().unwrap() = Some((target_id, std::time::Instant::now()));

                    match route {
                        RouteMode::Direct => direct_click(x, y, vw, vh).map_err(|e| e.to_string()),
                        RouteMode::Ghost => {
                            ghost_click(hwnd, x, y).map_err(|e| e.to_string())
                        }
                    }
                } else {
                    Err(format!("Node {} not found in scene graph", target_id))
                }
            }
            AgentAction::Type { text, .. } => {
                if let Some((x, y)) = coords {
                    // Click into target field first to focus the cursor
                    let _ = direct_click(x, y, vw, vh);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                direct_type(&text).map_err(|e| e.to_string())
            }
            AgentAction::Scroll { delta_y, .. } => {
                if let Some((x, y)) = coords {
                    direct_scroll(x, y, delta_y, vw, vh).map_err(|e| e.to_string())
                } else {
                    Err(format!("Node {} not found in scene graph", target_id))
                }
            }
            AgentAction::Key { vk, .. } => {
                direct_key(vk).map_err(|e| e.to_string())
            }
            AgentAction::Focus { .. } => {
                if let Some((x, y)) = coords {
                    direct_click(x, y, vw, vh).map_err(|e| e.to_string())
                } else {
                    Err(format!("Node {} not found in scene graph", target_id))
                }
            }
            AgentAction::Learn { .. } => unreachable!(),
        };

        ActionResult {
            action: action_name.to_string(),
            route,
            target_id,
            coords,
            ok: result.is_ok(),
            error: result.err(),
        }
    }
}

fn find_node_mut(node: &mut vact_protocol::SceneNode, target_id: u32) -> Option<&mut vact_protocol::SceneNode> {
    if node.id == target_id {
        return Some(node);
    }
    for child in &mut node.children {
        if let Some(found) = find_node_mut(child, target_id) {
            return Some(found);
        }
    }
    None
}

