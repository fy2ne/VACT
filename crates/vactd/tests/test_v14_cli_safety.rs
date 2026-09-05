//! Unit & Integration Tests for V14 — CLI, Logging & Safety Controls

use vactd::cli::{parse_args, Command};
use vactd::io_bus::{AgentAction, IoBus};

#[test]
fn test_cli_parse_default_start() {
    let args = vec!["vactd".to_string()];
    let cmd = parse_args(&args);
    assert_eq!(
        cmd,
        Command::Start {
            overlay: false,
            json_log: false,
            pipe_name: "\\\\.\\pipe\\VACT".to_string(),
        }
    );
}

#[test]
fn test_cli_parse_start_with_overlay_and_json() {
    let args = vec![
        "vactd".to_string(),
        "start".to_string(),
        "--overlay".to_string(),
        "--json-log".to_string(),
    ];
    let cmd = parse_args(&args);
    assert_eq!(
        cmd,
        Command::Start {
            overlay: true,
            json_log: true,
            pipe_name: "\\\\.\\pipe\\VACT".to_string(),
        }
    );
}

#[test]
fn test_cli_parse_once_mode() {
    let args = vec!["vactd".to_string(), "once".to_string()];
    let cmd = parse_args(&args);
    assert_eq!(cmd, Command::Once { overlay: false });

    let args_alias = vec!["vactd".to_string(), "capture-tree".to_string(), "--overlay".to_string()];
    let cmd_alias = parse_args(&args_alias);
    assert_eq!(cmd_alias, Command::Once { overlay: true });
}

#[test]
fn test_cli_parse_status() {
    let args = vec![
        "vactd".to_string(),
        "status".to_string(),
        "--pipe".to_string(),
        "\\\\.\\pipe\\custom".to_string(),
    ];
    let cmd = parse_args(&args);
    assert_eq!(
        cmd,
        Command::Status {
            pipe_name: "\\\\.\\pipe\\custom".to_string(),
        }
    );
}

#[test]
fn test_cli_parse_bench() {
    let args = vec![
        "vactd".to_string(),
        "bench".to_string(),
        "-i".to_string(),
        "250".to_string(),
        "--no-readback".to_string(),
    ];
    let cmd = parse_args(&args);
    assert_eq!(
        cmd,
        Command::Bench {
            iterations: 250,
            test_readback: false,
        }
    );
}

#[test]
fn test_io_bus_emergency_stop_blocks_actions() {
    let io_bus = IoBus::new();
    assert!(!io_bus.is_emergency_stopped());

    // Trigger emergency stop (F12)
    io_bus.trigger_emergency_stop();
    assert!(io_bus.is_emergency_stopped());

    // Dispatching any action must be immediately blocked
    let res = io_bus.dispatch(AgentAction::Click { target_id: 1 });
    assert!(!res.ok);
    assert!(res.error.is_some());
    assert!(res.error.unwrap().contains("Emergency Stop"));

    // Reset emergency stop (Ctrl+F12)
    io_bus.reset_emergency_stop();
    assert!(!io_bus.is_emergency_stopped());
}
