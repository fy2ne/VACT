//! Tests for taskbar app resolution and scene graph enrichment.

use vact_protocol::{NodeType, SceneGraph, SceneNode, Viewport};
use vactd::taskbar::{enrich_scene_graph_taskbar, enrich_scene_graph_with_apps, resolve_app_name, AppInfo};

#[test]
fn test_resolve_app_name_known_binaries() {
    assert_eq!(resolve_app_name("discord.exe", ""), "Discord");
    assert_eq!(resolve_app_name("Discord.EXE", ""), "Discord");
    assert_eq!(resolve_app_name("chrome.exe", ""), "Google Chrome");
    assert_eq!(resolve_app_name("Code.exe", ""), "Visual Studio Code");
    assert_eq!(resolve_app_name("spotify.exe", ""), "Spotify");
    assert_eq!(resolve_app_name("notepad.exe", ""), "Notepad");
    assert_eq!(resolve_app_name("msedge.exe", ""), "Microsoft Edge");
    assert_eq!(resolve_app_name("explorer.exe", ""), "File Explorer");
    assert_eq!(resolve_app_name("calc.exe", ""), "Calculator");
}

#[test]
fn test_resolve_app_name_window_title_fallback() {
    assert_eq!(
        resolve_app_name("custom_client.exe", "Project Workspace - Custom Studio"),
        "Custom Studio"
    );
    assert_eq!(
        resolve_app_name("my_tool.exe", "General"),
        "My_tool"
    );
}

#[test]
fn test_taskbar_enrichment_labels_icons() {
    let mut root = SceneNode::container(0, [0, 0, 1920, 1080]);

    // Start button
    let start_btn = SceneNode::container(1, [10, 1040, 40, 1070]);
    // Search button
    let search_btn = SceneNode::container(2, [60, 1040, 90, 1070]);
    // Taskbar app icon 1 (e.g. Discord / Chrome)
    let mut app_icon1 = SceneNode::container(3, [110, 1040, 145, 1070]);
    app_icon1.node_type = NodeType::Icon;
    // Taskbar app icon 2
    let mut app_icon2 = SceneNode::container(4, [155, 1040, 190, 1070]);
    app_icon2.node_type = NodeType::Icon;

    root.children.push(start_btn);
    root.children.push(search_btn);
    root.children.push(app_icon1);
    root.children.push(app_icon2);

    let mut graph = SceneGraph::new(
        1,
        1000,
        Viewport { width: 1920, height: 1080, scale_factor: 1.0 },
        Some("Desktop".to_string()),
        root,
    );

    enrich_scene_graph_taskbar(&mut graph);

    // Check Start button
    assert_eq!(graph.root.children[0].label.as_deref(), Some("Start"));
    assert_eq!(graph.root.children[0].interactable, Some(true));

    // Check Search button
    assert_eq!(graph.root.children[1].label.as_deref(), Some("Search"));
    assert_eq!(graph.root.children[1].interactable, Some(true));

    // Check App icons are interactable and enriched if running apps exist
    assert_eq!(graph.root.children[2].interactable, Some(true));
    assert_eq!(graph.root.children[3].interactable, Some(true));
}

#[test]
fn test_taskbar_enrichment_with_mock_apps() {
    let mut root = SceneNode::container(0, [0, 0, 1920, 1080]);
    let mut discord_icon = SceneNode::container(1, [105, 1040, 140, 1070]);
    discord_icon.node_type = NodeType::Icon;
    let mut chrome_icon = SceneNode::container(2, [150, 1040, 185, 1070]);
    chrome_icon.node_type = NodeType::Icon;

    root.children.push(discord_icon);
    root.children.push(chrome_icon);

    let mut graph = SceneGraph::new(
        1,
        1000,
        Viewport { width: 1920, height: 1080, scale_factor: 1.0 },
        Some("Desktop".to_string()),
        root,
    );

    let mock_apps = vec![
        AppInfo {
            process_name: "discord.exe".to_string(),
            window_title: "Discord".to_string(),
            app_name: "Discord".to_string(),
        },
        AppInfo {
            process_name: "chrome.exe".to_string(),
            window_title: "Google Chrome".to_string(),
            app_name: "Google Chrome".to_string(),
        },
    ];

    enrich_scene_graph_with_apps(&mut graph, &mock_apps);

    assert_eq!(graph.root.children[0].label.as_deref(), Some("Discord"));
    assert_eq!(graph.root.children[0].interactable, Some(true));

    assert_eq!(graph.root.children[1].label.as_deref(), Some("Google Chrome"));
    assert_eq!(graph.root.children[1].interactable, Some(true));
}

