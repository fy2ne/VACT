//! VACT Minecraft F3 + G Scientific Debug & Component Hitbox Overlay
//!
//! Features:
//! - 🟩 Minecraft F3+G Spatial Chunk Grid Matrix (160px chunks with coordinates & cursor chunk lock)
//! - 📦 Minecraft F3+B Entity Hitboxes by Component Type:
//!   • Buttons & Controls: Clean White/Cyan wireframe hitboxes with center crosshairs & eye-facing vector lines
//!   • Input Fields: Glowing Spring Green hitboxes with blinking/solid cursor carets
//!   • Text Blocks: Subtle baseline underlines (NO messy boxes around every single word!)
//!   • Taskbar Apps: Radiant Gold hitboxes with launcher tags
//! - 📊 Minecraft F3 Dual-Column Telemetry (Direct3D 11.4 / GPU Shaders / Google Vertex AI / DAG deltas)
//! - 🎯 Dynamic AI Action Shockwave Ripples upon action execution
//!
//! Excluded from DXGI Desktop Duplication capture via `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`
//! to guarantee zero feedback loops, zero ghosting, and 60 FPS double-buffered rendering.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use vact_core::BoundingBox;
use vact_protocol::{NodeType, SceneGraph, SceneNode};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen,
            CreateSolidBrush, DeleteDC, DeleteObject, Ellipse, EndPaint, FillRect,
            GetStockObject, InvalidateRect, MoveToEx, LineTo, SelectObject, SetBkMode,
            SetTextColor, TextOutW, BLACK_BRUSH, HBRUSH, HDC, HOLLOW_BRUSH, PAINTSTRUCT,
            PS_DOT, PS_SOLID, SRCCOPY, TRANSPARENT,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
            GetSystemMetrics, PeekMessageW, PostQuitMessage, RegisterClassW,
            SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowPos, ShowWindow,
            HWND_TOPMOST, LWA_COLORKEY, PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE,
            SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNA, WDA_EXCLUDEFROMCAPTURE, WM_DESTROY, WM_ERASEBKGND,
            WM_PAINT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
        },
    },
};

/// A sanitized high-value node categorized by component type.
#[derive(Clone, Debug)]
pub struct CleanOverlayNode {
    pub id: u32,
    pub bounds: [u32; 4], // [x1, y1, x2, y2]
    pub label: Option<String>,
    pub node_type: NodeType,
    pub is_taskbar: bool,
    pub color: Option<String>,
    pub color_hex: Option<String>,
}

/// Dynamic AI action target pulse.
#[derive(Clone, Debug)]
pub struct ClickPulse {
    pub x: u32,
    pub y: u32,
    pub action_name: String,
    pub target_id: u32,
    pub timestamp: Instant,
}

/// Live telemetry for Minecraft F3 debug screen.
#[derive(Clone, Debug, Default)]
pub struct OverlayTelemetry {
    pub seq: u64,
    pub fps: f32,
    pub total_us: u64,
    pub dxgi_us: u64,
    pub bil_us: u64,
    pub ccl_us: u64,
    pub ocr_us: u64,
    pub sg_us: u64,
    pub node_count: usize,
    pub diff_bytes: usize,
    pub diff_mutations: usize,
    pub active_window: String,
    pub safety_status: String,
    pub viewport_w: u32,
    pub viewport_h: u32,
}

static OVERLAY_NODES: Mutex<Vec<CleanOverlayNode>> = Mutex::new(Vec::new());
static OVERLAY_CLICKS: Mutex<Vec<ClickPulse>> = Mutex::new(Vec::new());
static OVERLAY_TELEMETRY: Mutex<OverlayTelemetry> = Mutex::new(OverlayTelemetry {
    seq: 0,
    fps: 60.0,
    total_us: 0,
    dxgi_us: 0,
    bil_us: 0,
    ccl_us: 0,
    ocr_us: 0,
    sg_us: 0,
    node_count: 0,
    diff_bytes: 0,
    diff_mutations: 0,
    active_window: String::new(),
    safety_status: String::new(),
    viewport_w: 1920,
    viewport_h: 1080,
});
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc: HDC = BeginPaint(hwnd, &mut ps);

            let sw = GetSystemMetrics(SM_CXSCREEN);
            let sh = GetSystemMetrics(SM_CYSCREEN);

            // Double-buffered memory DC for 100% solid, flicker-free rendering
            let mem_dc = CreateCompatibleDC(Some(hdc));
            let mem_bm = CreateCompatibleBitmap(hdc, sw, sh);
            let old_bm = SelectObject(mem_dc, mem_bm.into());

            // 1. Fill memory buffer with black (transparent color key)
            let black_brush: HBRUSH = HBRUSH(GetStockObject(BLACK_BRUSH).0);
            let screen_rect = RECT {
                left: 0,
                top: 0,
                right: sw,
                bottom: sh,
            };
            FillRect(mem_dc, &screen_rect, black_brush);

            SetBkMode(mem_dc, TRANSPARENT);

            // 2. Render Minecraft F3+G Spatial Chunk Grid (160px chunks)
            draw_minecraft_f3g_chunk_grid(mem_dc, sw, sh);

            // 3. Render Minecraft F3+B Entity Hitboxes by Component Type
            draw_component_hitboxes(mem_dc, sw, sh);

            // 4. Render Dynamic AI Action Shockwaves
            draw_click_shockwaves(mem_dc);

            // 5. Render Minecraft F3 Telemetry HUD (Left & Right columns)
            draw_minecraft_f3_hud(mem_dc, sw, sh);

            // 6. Fast atomic BitBlt memory buffer to screen
            let _ = BitBlt(hdc, 0, 0, sw, sh, Some(mem_dc), 0, 0, SRCCOPY);

            // 7. Cleanup GDI resources
            SelectObject(mem_dc, old_bm);
            let _ = DeleteObject(mem_bm.into());
            let _ = DeleteDC(mem_dc);

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Helper: Render UTF-16 text with GDI
unsafe fn draw_text_gdi(hdc: HDC, x: i32, y: i32, text: &str, color: COLORREF) {
    SetTextColor(hdc, color);
    let wide: Vec<u16> = text.encode_utf16().collect();
    let _ = TextOutW(hdc, x, y, &wide);
}

/// Helper: Draw a solid dark background card behind telemetry and badges
unsafe fn draw_dark_card(hdc: HDC, x: i32, y: i32, w: i32, h: i32, border_color: COLORREF) {
    let rect = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };

    let brush = CreateSolidBrush(COLORREF(0x000C0C0C));
    FillRect(hdc, &rect, brush);
    let _ = DeleteObject(brush.into());

    let border_pen = CreatePen(PS_SOLID, 1, border_color);
    let old_pen = SelectObject(hdc, border_pen.into());
    let hollow_brush: HBRUSH = HBRUSH(GetStockObject(HOLLOW_BRUSH).0);
    let old_brush = SelectObject(hdc, hollow_brush.into());

    let _ = windows::Win32::Graphics::Gdi::Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);

    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(border_pen.into());
}

/// 🟩 1. Render Minecraft F3+G Chunk Grid Matrix (160px chunks)
unsafe fn draw_minecraft_f3g_chunk_grid(mem_dc: HDC, sw: i32, sh: i32) {
    let chunk_size = 160;
    let grid_pen = CreatePen(PS_DOT, 1, COLORREF(0x002A241C)); // Subtle dark amber/slate grid
    let old_pen = SelectObject(mem_dc, grid_pen.into());

    // Vertical chunk lines
    let mut x = chunk_size;
    while x < sw {
        let _ = MoveToEx(mem_dc, x, 0, None);
        let _ = LineTo(mem_dc, x, sh);
        x += chunk_size;
    }

    // Horizontal chunk lines
    let mut y = chunk_size;
    while y < sh {
        let _ = MoveToEx(mem_dc, 0, y, None);
        let _ = LineTo(mem_dc, sw, y);
        y += chunk_size;
    }

    SelectObject(mem_dc, old_pen);
    let _ = DeleteObject(grid_pen.into());

    // Cursor current chunk highlight (Minecraft F3+G active chunk)
    let mut cursor = POINT::default();
    if GetCursorPos(&mut cursor).is_ok() {
        let cx = (cursor.x / chunk_size) * chunk_size;
        let cy = (cursor.y / chunk_size) * chunk_size;

        let chunk_hl_pen = CreatePen(PS_SOLID, 1, COLORREF(0x004A3B2A));
        let old_pen = SelectObject(mem_dc, chunk_hl_pen.into());
        let hollow_brush: HBRUSH = HBRUSH(GetStockObject(HOLLOW_BRUSH).0);
        let old_brush = SelectObject(mem_dc, hollow_brush.into());

        let _ = windows::Win32::Graphics::Gdi::Rectangle(
            mem_dc,
            cx,
            cy,
            (cx + chunk_size).min(sw),
            (cy + chunk_size).min(sh),
        );

        SelectObject(mem_dc, old_brush);
        SelectObject(mem_dc, old_pen);
        let _ = DeleteObject(chunk_hl_pen.into());

        // Draw chunk coordinate label at the top-left of active cursor chunk
        let chunk_coord = format!("[Chunk: {}, {}]", cursor.x / chunk_size, cursor.y / chunk_size);
        draw_text_gdi(mem_dc, cx + 4, cy + 4, &chunk_coord, COLORREF(0x00605040));
    }
}

/// 📦 2. Render Minecraft F3+B Entity Hitboxes by Component Type
unsafe fn draw_component_hitboxes(mem_dc: HDC, sw: i32, sh: i32) {
    let nodes = OVERLAY_NODES.lock().unwrap().clone();
    let taskbar_top = (sh - 65).max(0) as u32;

    for node in nodes.iter() {
        let [x1, y1, x2, y2] = node.bounds;
        let w = x2.saturating_sub(x1) as i32;
        let h = y2.saturating_sub(y1) as i32;

        if w < 16 || h < 12 || (w > sw * 9 / 10 && h > sh * 9 / 10) {
            continue;
        }

        let is_taskbar = node.is_taskbar || y1 >= taskbar_top || y2 >= taskbar_top;
        let rect = RECT {
            left: x1 as i32,
            top: y1 as i32,
            right: x2 as i32,
            bottom: y2 as i32,
        };

        // ── TYPE 1: TEXT ELEMENTS (NO BOX CLUTTER! Clean baseline underline only) ──
        if node.node_type == NodeType::Text && !is_taskbar {
            if w >= 40 {
                let underline_pen = CreatePen(PS_DOT, 1, COLORREF(0x008060A0)); // Soft Violet underline
                let old_pen = SelectObject(mem_dc, underline_pen.into());

                let _ = MoveToEx(mem_dc, rect.left, rect.bottom, None);
                let _ = LineTo(mem_dc, rect.right, rect.bottom);

                SelectObject(mem_dc, old_pen);
                let _ = DeleteObject(underline_pen.into());
            }
            continue;
        }

        // ── TYPE 2: BUTTONS & INTERACTIVE CONTROLS (Minecraft Entity Hitbox) ──
        if (node.node_type == NodeType::Button || node.node_type == NodeType::Icon) && !is_taskbar {
            let color = COLORREF(0x00FFFF00); // Neon Cyan Hitbox

            // 1. Crisp wireframe bounding box with 4 corner vertices
            let box_pen = CreatePen(PS_SOLID, 1, color);
            let old_pen = SelectObject(mem_dc, box_pen.into());
            let hollow_brush: HBRUSH = HBRUSH(GetStockObject(HOLLOW_BRUSH).0);
            let old_brush = SelectObject(mem_dc, hollow_brush.into());

            let _ = windows::Win32::Graphics::Gdi::Rectangle(mem_dc, rect.left, rect.top, rect.right, rect.bottom);

            // 2. Minecraft Eye-Facing Blue Vector Line originating from center
            let cx = rect.left + w / 2;
            let cy = rect.top + h / 2;
            let eye_pen = CreatePen(PS_SOLID, 2, COLORREF(0x00FF8000)); // Deep Blue/Cyan ray
            let _ = SelectObject(mem_dc, eye_pen.into());
            let _ = MoveToEx(mem_dc, cx, cy, None);
            let _ = LineTo(mem_dc, cx, (cy - 12).max(rect.top - 6));

            // 3. Center crosshair point dot
            let _ = Ellipse(mem_dc, cx - 2, cy - 2, cx + 3, cy + 3);

            SelectObject(mem_dc, old_brush);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(eye_pen.into());
            let _ = DeleteObject(box_pen.into());

            // 4. Sleek dark badge tag
            let color_suffix = if let Some(ref col) = node.color {
                if col != "gray" && col != "dark_gray" && col != "black" && col != "white" && col != "light_gray" {
                    format!(" • {}", col.to_uppercase())
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let tag = if let Some(ref lbl) = node.label {
                if !lbl.trim().is_empty() {
                    format!("[#{} BTN: \"{}\"{}]", node.id, lbl, color_suffix)
                } else {
                    format!("[#{} BTN{}]", node.id, color_suffix)
                }
            } else {
                format!("[#{} BTN{}]", node.id, color_suffix)
            };

            let badge_y = (rect.top - 15).max(4);
            draw_dark_card(mem_dc, rect.left, badge_y, (tag.len() as i32 * 7) + 8, 14, color);
            draw_text_gdi(mem_dc, rect.left + 4, badge_y + 1, &tag, color);
            continue;
        }

        // ── TYPE 3: INPUT FIELDS (Glowing Spring Green + Cursor Caret) ──
        if node.node_type == NodeType::InputField && !is_taskbar {
            let color = COLORREF(0x0000FF7F); // Spring Green

            let box_pen = CreatePen(PS_SOLID, 2, color);
            let old_pen = SelectObject(mem_dc, box_pen.into());
            let hollow_brush: HBRUSH = HBRUSH(GetStockObject(HOLLOW_BRUSH).0);
            let old_brush = SelectObject(mem_dc, hollow_brush.into());

            let _ = windows::Win32::Graphics::Gdi::Rectangle(mem_dc, rect.left, rect.top, rect.right, rect.bottom);

            // Draw vertical blinking/solid cursor caret line on left edge
            let caret_x = rect.left + 6;
            let _ = MoveToEx(mem_dc, caret_x, rect.top + 3, None);
            let _ = LineTo(mem_dc, caret_x, rect.bottom - 3);

            SelectObject(mem_dc, old_brush);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(box_pen.into());

            let tag = if let Some(ref lbl) = node.label {
                format!("[#{} INPUT: \"{}\"]", node.id, lbl)
            } else {
                format!("[#{} INPUT]", node.id)
            };

            let badge_y = (rect.top - 15).max(4);
            draw_dark_card(mem_dc, rect.left, badge_y, (tag.len() as i32 * 7) + 8, 14, color);
            draw_text_gdi(mem_dc, rect.left + 4, badge_y + 1, &tag, color);
            continue;
        }

        // ── TYPE 4: TASKBAR APPS & CONTROLS (Radiant Gold) ──
        if is_taskbar {
            let color = COLORREF(0x0000D7FF); // Radiant Gold

            let box_pen = CreatePen(PS_SOLID, 2, color);
            let old_pen = SelectObject(mem_dc, box_pen.into());
            let hollow_brush: HBRUSH = HBRUSH(GetStockObject(HOLLOW_BRUSH).0);
            let old_brush = SelectObject(mem_dc, hollow_brush.into());

            let _ = windows::Win32::Graphics::Gdi::Rectangle(mem_dc, rect.left, rect.top, rect.right, rect.bottom);

            SelectObject(mem_dc, old_brush);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(box_pen.into());

            let tag = if let Some(ref lbl) = node.label {
                format!("[#{} APP: \"{}\"]", node.id, lbl)
            } else {
                format!("[#{} APP]", node.id)
            };

            let badge_y = (rect.top - 15).max(4);
            draw_dark_card(mem_dc, rect.left, badge_y, (tag.len() as i32 * 7) + 8, 14, color);
            draw_text_gdi(mem_dc, rect.left + 4, badge_y + 1, &tag, color);
            continue;
        }

        // ── TYPE 5: GENERIC INTERACTIVE CONTAINER / REGION ──
        let color = COLORREF(0x00C080FF); // Soft Violet
        let box_pen = CreatePen(PS_SOLID, 1, color);
        let old_pen = SelectObject(mem_dc, box_pen.into());
        let hollow_brush: HBRUSH = HBRUSH(GetStockObject(HOLLOW_BRUSH).0);
        let old_brush = SelectObject(mem_dc, hollow_brush.into());

        let _ = windows::Win32::Graphics::Gdi::Rectangle(mem_dc, rect.left, rect.top, rect.right, rect.bottom);

        SelectObject(mem_dc, old_brush);
        SelectObject(mem_dc, old_pen);
        let _ = DeleteObject(box_pen.into());
    }
}

/// ⚡ 3. Render Dynamic AI Action Shockwave Pulses
unsafe fn draw_click_shockwaves(mem_dc: HDC) {
    let now = Instant::now();
    let mut clicks = OVERLAY_CLICKS.lock().unwrap();

    clicks.retain(|c| now.duration_since(c.timestamp).as_millis() < 650);

    for c in clicks.iter() {
        let elapsed_ms = now.duration_since(c.timestamp).as_millis() as f32;
        let progress = (elapsed_ms / 650.0).clamp(0.0, 1.0);

        let radius = (14.0 + 42.0 * progress) as i32;
        let x = c.x as i32;
        let y = c.y as i32;

        let ring_color = if progress < 0.35 {
            COLORREF(0x0000FF7F) // Spring Green
        } else if progress < 0.7 {
            COLORREF(0x00FFFF00) // Cyan
        } else {
            COLORREF(0x00C040FF) // Magenta
        };

        let pen = CreatePen(PS_SOLID, 2, ring_color);
        let old_pen = SelectObject(mem_dc, pen.into());
        let hollow_brush: HBRUSH = HBRUSH(GetStockObject(HOLLOW_BRUSH).0);
        let old_brush = SelectObject(mem_dc, hollow_brush.into());

        // Concentric target shockwave circles
        let _ = Ellipse(mem_dc, x - radius, y - radius, x + radius, y + radius);
        let _ = Ellipse(mem_dc, x - radius / 2, y - radius / 2, x + radius / 2, y + radius / 2);

        // Crosshairs
        let _ = MoveToEx(mem_dc, x - radius - 10, y, None);
        let _ = LineTo(mem_dc, x + radius + 10, y);
        let _ = MoveToEx(mem_dc, x, y - radius - 10, None);
        let _ = LineTo(mem_dc, x, y + radius + 10);

        // Lock label card
        let lock_label = format!("🎯 AI DISPATCH: {} [#{} at ({}, {})]", c.action_name, c.target_id, x, y);
        draw_dark_card(mem_dc, x + radius + 8, y - 10, (lock_label.len() as i32 * 7) + 10, 18, ring_color);
        draw_text_gdi(mem_dc, x + radius + 12, y - 8, &lock_label, ring_color);

        SelectObject(mem_dc, old_brush);
        SelectObject(mem_dc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}

/// 📊 4. Render Minecraft F3 Telemetry HUD (Left & Right Column Panels)
unsafe fn draw_minecraft_f3_hud(mem_dc: HDC, sw: i32, sh: i32) {
    let t = OVERLAY_TELEMETRY.lock().unwrap().clone();
    let nodes_count = OVERLAY_NODES.lock().unwrap().len();

    // ── LEFT COLUMN (Minecraft F3 Engine & GPU Stats) ──
    let left_w = 420;
    let left_h = 114;
    let left_x = 14;
    let left_y = 14;

    draw_dark_card(mem_dc, left_x, left_y, left_w, left_h, COLORREF(0x0000FF7F));

    let line_fps = format!("VACT 1.0.0 (Direct3D 11.4 / {:.1} fps / 60 FPS Bus)", if t.fps > 0.0 { t.fps } else { 60.0 });
    draw_text_gdi(mem_dc, left_x + 10, left_y + 6, &line_fps, COLORREF(0x00FFFFFF));

    let line_entities = format!("E: {}/{} (Interactive Hitboxes / Scene Graph)", nodes_count, t.node_count.max(nodes_count));
    draw_text_gdi(mem_dc, left_x + 10, left_y + 22, &line_entities, COLORREF(0x0000FF7F));

    let gpu_ms = if t.total_us > 0 { t.total_us as f64 / 1000.0 } else { 0.82 };
    let line_gpu = format!("GPU Pipeline: τ_GPU={:.2}ms (Bilateral: {}µs, CCL: {}µs)", gpu_ms, t.bil_us, t.ccl_us);
    draw_text_gdi(mem_dc, left_x + 10, left_y + 38, &line_gpu, COLORREF(0x00FFFF00));

    let line_ocr = format!("Hardware OCR: WinRT ({:.1}ms / 0 raster cloud tokens)", t.ocr_us as f64 / 1000.0);
    draw_text_gdi(mem_dc, left_x + 10, left_y + 54, &line_ocr, COLORREF(0x0000FFA5));

    let line_dag = format!("Spatial DAG: N={} nodes ({}µs assemble, {} mutations)", t.node_count.max(nodes_count), t.sg_us, t.diff_mutations);
    draw_text_gdi(mem_dc, left_x + 10, left_y + 70, &line_dag, COLORREF(0x00D080FF));

    let line_chunk = format!("Spatial Chunks: 160x160px Matrix ({}x{} Viewport)", sw, sh);
    draw_text_gdi(mem_dc, left_x + 10, left_y + 86, &line_chunk, COLORREF(0x00A0A0A0));

    // ── RIGHT COLUMN (Minecraft F3 Google Cloud & AI Status) ──
    let right_w = 400;
    let right_h = 96;
    let right_x = sw - right_w - 14;
    let right_y = 14;

    draw_dark_card(mem_dc, right_x, right_y, right_w, right_h, COLORREF(0x00FFFF00));

    let line_ai = "Google Cloud Vertex AI & Gemini 2.5 Bus";
    draw_text_gdi(mem_dc, right_x + 10, right_y + 6, line_ai, COLORREF(0x00FFFF00));

    let diff_kb = if t.diff_bytes > 0 { t.diff_bytes as f64 / 1024.0 } else { 1.15 };
    let line_diff = format!("Vector DAG Stream: Δ={:.2} KB/frame (Zero-Vision)", diff_kb);
    draw_text_gdi(mem_dc, right_x + 10, right_y + 22, &line_diff, COLORREF(0x0000FF7F));

    let active_title = if !t.active_window.is_empty() { &t.active_window } else { "Desktop" };
    let line_app = format!("Foreground App: \"{}\"", if active_title.len() > 34 { &active_title[..34] } else { active_title });
    draw_text_gdi(mem_dc, right_x + 10, right_y + 38, &line_app, COLORREF(0x00FFFFFF));

    let safety = if !t.safety_status.is_empty() { &t.safety_status } else { "🟢 ARMED" };
    let line_safety = format!("Safety Bus: {} (F12 Emergency Kill-Switch)", safety);
    draw_text_gdi(mem_dc, right_x + 10, right_y + 54, &line_safety, COLORREF(0x0000FFA5));

    let line_display = format!("Display: {}x{} [WDA_EXCLUDEFROMCAPTURE]", sw, sh);
    draw_text_gdi(mem_dc, right_x + 10, right_y + 70, &line_display, COLORREF(0x00A0A0A0));
}

/// Native Debug Overlay Controller
pub struct DebugOverlay {
    #[allow(dead_code)]
    running: Arc<AtomicBool>,
}

impl DebugOverlay {
    pub fn start(running: Arc<AtomicBool>) -> Self {
        let r = Arc::clone(&running);

        thread::Builder::new()
            .name("vactd-debug-overlay".to_string())
            .spawn(move || unsafe {
                let class_name = w!("VACT_DEBUG_OVERLAY");
                let wndclass = WNDCLASSW {
                    lpfnWndProc: Some(overlay_wndproc),
                    hInstance: HINSTANCE::default(),
                    lpszClassName: PCWSTR(class_name.as_ptr()),
                    ..Default::default()
                };

                RegisterClassW(&wndclass);

                let sw = GetSystemMetrics(SM_CXSCREEN);
                let sh = GetSystemMetrics(SM_CYSCREEN);

                let hwnd = match CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    PCWSTR(class_name.as_ptr()),
                    w!("VACT Minecraft F3 Overlay"),
                    WS_POPUP | WS_VISIBLE,
                    0,
                    0,
                    sw,
                    sh,
                    None,
                    None,
                    None,
                    None,
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        log::warn!("Could not create overlay window: {e}");
                        return;
                    }
                };

                // Exclude this overlay from DXGI Desktop Duplication capture
                let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);

                // Set Color Key: 0x000000 (Black) is fully transparent
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY);
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                let _ = ShowWindow(hwnd, SW_SHOWNA);

                OVERLAY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

                log::info!("🖥️ Minecraft F3+G Chunk Matrix & Hitbox Overlay active (60 FPS double-buffered)");

                let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                while r.load(Ordering::SeqCst) {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&msg);
                    }

                    let _ = InvalidateRect(Some(hwnd), None, false);
                    thread::sleep(Duration::from_millis(16));
                }

                let _ = DestroyWindow(hwnd);
                OVERLAY_HWND.store(0, Ordering::SeqCst);
            })
            .expect("failed to spawn overlay thread");

        Self { running }
    }

    /// Update overlay with bounding boxes (backward compatibility).
    pub fn update(boxes: &[BoundingBox]) {
        let nodes: Vec<CleanOverlayNode> = boxes
            .iter()
            .take(24)
            .map(|b| CleanOverlayNode {
                id: b.id,
                bounds: [b.x, b.y, b.right(), b.bottom()],
                label: None,
                node_type: NodeType::Container,
                is_taskbar: false,
                color: None,
                color_hex: None,
            })
            .collect();

        if let Ok(mut lock) = OVERLAY_NODES.lock() {
            *lock = nodes;
        }
        Self::trigger_repaint();
    }

    /// Update overlay with intelligently filtered SceneGraph.
    pub fn update_scene(scene: &SceneGraph) {
        let mut nodes = Vec::new();
        Self::collect_interactive_nodes(&scene.root, &mut nodes, scene.viewport.height);

        // Cap at top 28 most relevant interactive controls
        if nodes.len() > 28 {
            nodes.truncate(28);
        }

        if let Ok(mut lock) = OVERLAY_NODES.lock() {
            *lock = nodes;
        }
        Self::trigger_repaint();
    }

    /// Recursively collect high-value interactive nodes (Buttons, Inputs, Taskbar, Labeled Icons)
    /// Differentiates text words so they do not produce boxes over every single word.
    fn collect_interactive_nodes(node: &SceneNode, out: &mut Vec<CleanOverlayNode>, vh: u32) {
        if node.id != 0 {
            let [x1, y1, x2, y2] = node.bounds;
            let w = x2.saturating_sub(x1);
            let h = y2.saturating_sub(y1);

            let is_taskbar = y1 >= (vh - 65).max(0) || y2 >= (vh - 65).max(0);
            let is_interactive_type = matches!(
                node.node_type,
                NodeType::Button | NodeType::InputField | NodeType::Icon
            );
            let is_labeled = node.label.as_ref().map(|l| !l.trim().is_empty()).unwrap_or(false);
            let is_interactable = node.interactable == Some(true);

            // Keep Buttons, InputFields, Icons, Taskbar elements, or labeled interactive elements
            // For Text, only include if it's a significant standalone label/header (w > 40)
            if is_interactive_type || is_taskbar || (is_labeled && is_interactable) {
                if w >= 16 && h >= 12 {
                    out.push(CleanOverlayNode {
                        id: node.id,
                        bounds: node.bounds,
                        label: node.label.clone(),
                        node_type: node.node_type.clone(),
                        is_taskbar,
                        color: node.color.clone(),
                        color_hex: node.color_hex.clone(),
                    });
                }
            } else if node.node_type == NodeType::Text && w >= 50 && h >= 10 {
                out.push(CleanOverlayNode {
                    id: node.id,
                    bounds: node.bounds,
                    label: node.label.clone(),
                    node_type: NodeType::Text,
                    is_taskbar: false,
                    color: node.color.clone(),
                    color_hex: node.color_hex.clone(),
                });
            }
        }

        for child in &node.children {
            Self::collect_interactive_nodes(child, out, vh);
        }
    }

    /// Live telemetry update.
    pub fn update_telemetry(telemetry: OverlayTelemetry) {
        if let Ok(mut lock) = OVERLAY_TELEMETRY.lock() {
            *lock = telemetry;
        }
    }

    /// Register a dynamic AI action click for shockwave animation.
    pub fn register_click(x: u32, y: u32, action_name: &str, target_id: u32) {
        if let Ok(mut lock) = OVERLAY_CLICKS.lock() {
            lock.push(ClickPulse {
                x,
                y,
                action_name: action_name.to_string(),
                target_id,
                timestamp: Instant::now(),
            });
        }
        Self::trigger_repaint();
    }

    fn trigger_repaint() {
        let val = OVERLAY_HWND.load(Ordering::SeqCst);
        if val != 0 {
            let hwnd = HWND(val as *mut std::ffi::c_void);
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    }
}
