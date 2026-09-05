//! Taskbar and Process Semantic Enrichment for `vactd`.
//!
//! Inspects running application processes and top-level Win32 windows to
//! correlate anonymous taskbar bounding boxes (`NodeType::Icon` with `label: None`)
//! to real application names (e.g. "Discord", "Google Chrome", "Visual Studio Code").

use std::collections::HashSet;
use std::path::Path;
use vact_protocol::{NodeType, SceneGraph, SceneNode};
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible,
};

/// Information about an active desktop application window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub process_name: String,
    pub app_name: String,
    pub window_title: String,
}

/// Map an executable name (e.g. "discord.exe") to a friendly display name.
pub fn resolve_app_name(exe_name: &str, window_title: &str) -> String {
    let lower_exe = exe_name.to_lowercase();
    match lower_exe.as_str() {
        "discord.exe" => "Discord".to_string(),
        "chrome.exe" => "Google Chrome".to_string(),
        "code.exe" => "Visual Studio Code".to_string(),
        "spotify.exe" => "Spotify".to_string(),
        "msedge.exe" => "Microsoft Edge".to_string(),
        "notepad.exe" => "Notepad".to_string(),
        "calc.exe" | "calculatorapp.exe" | "calculator.exe" => "Calculator".to_string(),
        "explorer.exe" => "File Explorer".to_string(),
        "cmd.exe" | "windowsterminal.exe" | "powershell.exe" | "pwsh.exe" => "Terminal".to_string(),
        "javaw.exe" | "java.exe" | "minecraft.exe" | "minecraftlauncher.exe" => "Minecraft".to_string(),
        "slack.exe" => "Slack".to_string(),
        "steam.exe" | "steamservice.exe" => "Steam".to_string(),
        "obs64.exe" | "obs32.exe" => "OBS Studio".to_string(),
        "devenv.exe" => "Visual Studio".to_string(),
        "thunderbird.exe" => "Thunderbird".to_string(),
        "brave.exe" => "Brave Browser".to_string(),
        "firefox.exe" => "Firefox".to_string(),
        "telegram.exe" => "Telegram".to_string(),
        "vlc.exe" => "VLC Media Player".to_string(),
        _ => {
            // Check if window title has a clear app suffix (e.g. "... - App Name")
            if let Some(pos) = window_title.rfind(" - ") {
                let suffix = window_title[pos + 3..].trim();
                if !suffix.is_empty() && suffix.len() < 30 {
                    return suffix.to_string();
                }
            }

            // Fallback: strip extension and capitalize
            let stem = Path::new(exe_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(exe_name);

            let mut chars = stem.chars();
            match chars.next() {
                None => stem.to_string(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        }
    }
}

/// Query top-level running application windows on the desktop.
pub fn get_running_apps() -> Vec<AppInfo> {
    let mut apps: Vec<AppInfo> = Vec::new();

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let apps_ptr = lparam.0 as *mut Vec<AppInfo>;
        let apps = &mut *apps_ptr;

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        let title_len = GetWindowTextLengthW(hwnd);
        if title_len <= 0 {
            return BOOL(1);
        }

        let mut title_buf = vec![0u16; (title_len + 1) as usize];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        if len <= 0 {
            return BOOL(1);
        }
        let window_title = String::from_utf16_lossy(&title_buf[..len as usize]);

        // Filter out OS shell / background surfaces
        if window_title == "Program Manager"
            || window_title == "Settings"
            || window_title == "Windows Input Experience"
            || window_title == "Task Switching"
        {
            return BOOL(1);
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }

        if let Ok(process_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut path_buf = [0u16; 1024];
            let mut path_len = path_buf.len() as u32;
            if QueryFullProcessImageNameW(
                process_handle,
                PROCESS_NAME_FORMAT(0),
                windows::core::PWSTR(path_buf.as_mut_ptr()),
                &mut path_len,
            )
            .is_ok()
                && path_len > 0
            {
                let full_path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
                let exe_name = Path::new(&full_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                if !exe_name.is_empty() {
                    let app_name = resolve_app_name(&exe_name, &window_title);
                    apps.push(AppInfo {
                        process_name: exe_name,
                        app_name,
                        window_title,
                    });
                }
            }
        }

        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut apps as *mut _ as isize));
    }

    // Deduplicate apps by app_name preserving appearance order
    let mut seen = HashSet::new();
    apps.into_iter()
        .filter(|app| seen.insert(app.app_name.clone()))
        .collect()
}

/// Retrieve the taskbar rectangle if available.
pub fn get_taskbar_rect() -> Option<RECT> {
    let class_name: Vec<u16> = "Shell_TrayWnd\0".encode_utf16().collect();
    unsafe {
        if let Ok(hwnd) = FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR::null()) {
            if !hwnd.is_invalid() {
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_ok() {
                    return Some(rect);
                }
            }
        }
    }
    None
}

/// Enrich untextured taskbar nodes in the SceneGraph with semantic application names.
pub fn enrich_scene_graph_taskbar(graph: &mut SceneGraph) {
    let running_apps = get_running_apps();
    enrich_scene_graph_with_apps(graph, &running_apps);
}

/// Enrich untextured taskbar nodes using an explicit list of running applications.
pub fn enrich_scene_graph_with_apps(graph: &mut SceneGraph, running_apps: &[AppInfo]) {
    let vh = graph.viewport.height;
    let taskbar_top = get_taskbar_rect()
        .map(|r| r.top as u32)
        .unwrap_or_else(|| vh.saturating_sub(60));

    // First, collect references or pointers to all taskbar nodes to sort and enrich them
    enrich_tree(&mut graph.root, taskbar_top, running_apps);
}

fn enrich_tree(root: &mut SceneNode, taskbar_top: u32, running_apps: &[AppInfo]) {
    // 1. Traverse and classify static taskbar buttons (Start, Search)
    classify_static_taskbar_nodes(root, taskbar_top);

    // 2. Collect all unlabelled taskbar app icon / interactive nodes
    let mut app_nodes: Vec<*mut SceneNode> = Vec::new();
    collect_unlabelled_app_nodes(root, taskbar_top, &mut app_nodes);

    // 3. Sort horizontally (left-to-right by x1 coordinate)
    app_nodes.sort_by_key(|ptr| unsafe { (**ptr).bounds[0] });

    // 4. Assign running applications in sequence
    for (idx, ptr) in app_nodes.into_iter().enumerate() {
        let node = unsafe { &mut *ptr };
        node.node_type = NodeType::Icon;
        node.interactable = Some(true);

        if idx < running_apps.len() && node.label.is_none() {
            node.label = Some(running_apps[idx].app_name.clone());
        }
    }
}

fn classify_static_taskbar_nodes(node: &mut SceneNode, taskbar_top: u32) {
    let [x1, y1, x2, y2] = node.bounds;
    let is_taskbar = y1 >= taskbar_top || y2 >= taskbar_top;

    if is_taskbar {
        if node.node_type == NodeType::Icon || node.node_type == NodeType::Container || node.node_type == NodeType::Button {
            if x1 < 50 && node.label.is_none() {
                node.node_type = NodeType::Icon;
                node.label = Some("Start".to_string());
                node.interactable = Some(true);
            } else if x1 >= 50 && x2 <= 100 && node.label.is_none() {
                node.node_type = NodeType::Icon;
                node.label = Some("Search".to_string());
                node.interactable = Some(true);
            } else {
                node.interactable = Some(true);
            }
        }
    }

    for child in &mut node.children {
        classify_static_taskbar_nodes(child, taskbar_top);
    }
}

fn collect_unlabelled_app_nodes(
    node: &mut SceneNode,
    taskbar_top: u32,
    collected: &mut Vec<*mut SceneNode>,
) {
    let [x1, y1, _x2, y2] = node.bounds;
    let is_taskbar = y1 >= taskbar_top || y2 >= taskbar_top;

    if is_taskbar {
        // App icon candidates: taskbar items not already labeled as Start or Search
        let is_start = node.label.as_deref() == Some("Start") || (x1 < 50 && node.label.is_none());
        let is_search = node.label.as_deref() == Some("Search") || (x1 >= 50 && x1 <= 100 && node.label.is_none());

        if !is_start && !is_search {
            if node.node_type == NodeType::Icon
                || (node.node_type == NodeType::Container && node.children.is_empty())
            {
                collected.push(node as *mut SceneNode);
            }
        }
    }

    for child in &mut node.children {
        collect_unlabelled_app_nodes(child, taskbar_top, collected);
    }
}
