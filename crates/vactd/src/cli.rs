//! V14 — CLI Subcommands & Diagnostics (`vactd`)
//!
//! Provides command-line parsing, daemon health probes, and structured reporting.

use std::{
    io::{Read, Write},
    os::windows::fs::OpenOptionsExt,
    time::Instant,
};

use vact_protocol::ipc::IpcMessage;

/// Subcommands supported by the `vactd` binary.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Start the continuous perception daemon, Named Pipe server, and I/O bus.
    Start {
        overlay: bool,
        json_log: bool,
        pipe_name: String,
    },
    /// Capture a single frame, output AST JSON & debug PNGs, then exit.
    Once {
        overlay: bool,
    },
    /// Query the status of an already-running `vactd` daemon via Named Pipe probe.
    Status {
        pipe_name: String,
    },
    /// Run the GPU compute and OCR latency benchmark suite.
    Bench {
        iterations: usize,
        test_readback: bool,
    },
    /// Display help information.
    Help,
}

/// Parse command-line arguments into a typed [`Command`].
pub fn parse_args(args: &[String]) -> Command {
    if args.len() <= 1 {
        // Default when running `vactd` with no args is `start`
        return Command::Start {
            overlay: false,
            json_log: false,
            pipe_name: "\\\\.\\pipe\\VACT".to_string(),
        };
    }

    let subcmd = args[1].as_str();

    match subcmd {
        "start" => {
            let overlay = args.iter().any(|a| a == "--overlay" || a == "--f3" || a == "--hud" || a == "--scientific" || a == "--hitboxes");
            let json_log = args.iter().any(|a| a == "--json-log" || a == "--json");
            let pipe_name = args
                .iter()
                .position(|a| a == "--pipe")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "\\\\.\\pipe\\VACT".to_string());

            Command::Start {
                overlay,
                json_log,
                pipe_name,
            }
        }
        "once" | "capture-tree" | "--once" => {
            let overlay = args.iter().any(|a| a == "--overlay" || a == "--f3" || a == "--hud" || a == "--scientific" || a == "--hitboxes");
            Command::Once { overlay }
        }
        "status" => {
            let pipe_name = args
                .iter()
                .position(|a| a == "--pipe")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "\\\\.\\pipe\\VACT".to_string());

            Command::Status { pipe_name }
        }
        "bench" | "benchmark" | "--benchmark" => {
            let iterations = args
                .iter()
                .position(|a| a == "--iterations" || a == "-i")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(100);

            let test_readback = !args.iter().any(|a| a == "--no-readback");
            Command::Bench {
                iterations,
                test_readback,
            }
        }
        "help" | "--help" | "-h" => Command::Help,
        _ => {
            // Handle flags passed directly without subcommand (e.g. `vactd --overlay` or `vactd --f3`)
            let overlay = args.iter().any(|a| a == "--overlay" || a == "--f3" || a == "--hud" || a == "--scientific" || a == "--hitboxes");
            let json_log = args.iter().any(|a| a == "--json-log" || a == "--json");
            let pipe_name = args
                .iter()
                .position(|a| a == "--pipe")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "\\\\.\\pipe\\VACT".to_string());

            Command::Start {
                overlay,
                json_log,
                pipe_name,
            }
        }
    }
}

/// Print CLI banner and usage information.
pub fn print_help() {
    println!(
        r#"
🚀 VACT Daemon (`vactd`) — Vector Agent Context Transport

USAGE:
    vactd [COMMAND] [OPTIONS]

COMMANDS:
    start           Start the continuous daemon with Named Pipe server (Default)
    once            Capture 1 frame, extract scene graph, export debug artifacts & exit
    status          Probe the running vactd daemon and check IPC latency & viewport
    bench           Run the hardware GPU compute & OCR latency benchmark suite
    help            Print this help menu

OPTIONS:
    --overlay       Enable native Win32 clean sub-pixel hitbox & target-lock overlay
    --hitboxes      Alias for --overlay (shows UI hitboxes and AI action animations)
    --json-log      Enable structured JSON logging for terminal output
    --pipe <NAME>   Custom Named Pipe path (Default: \\.\pipe\VACT)
    -i, --iterations <N>  Number of iterations for benchmark suite (Default: 100)

SAFETY CONTROLS:
    [F12]           Global Emergency Stop (Immediately freezes synthetic agent input)
    [Ctrl + F12]    Reset Emergency Stop (Resumes synthetic agent input)
"#
    );
}

/// Probe a running `vactd` daemon via Named Pipe IPC and print diagnostic status.
pub fn probe_status(pipe_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Probing VACT daemon at pipe \"{}\"...", pipe_name);
    let start_t = Instant::now();

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(0)
        .open(pipe_name)
    {
        Ok(f) => f,
        Err(e) => {
            println!("\n❌ Status: OFFLINE");
            println!("   Could not connect to Named Pipe: {e}");
            println!("   Run \"vactd start\" in another terminal to boot the daemon.\n");
            return Ok(());
        }
    };

    let connect_us = start_t.elapsed().as_micros();

    // Send handshake
    let handshake = IpcMessage::Handshake {
        protocol: "VACT/1.0".to_string(),
        capabilities: vec!["STATUS_PROBE".to_string()],
    };
    let json_bytes = serde_json::to_vec(&handshake)?;
    let len = json_bytes.len() as u32;

    file.write_all(&len.to_le_bytes())?;
    file.write_all(&json_bytes)?;
    file.flush()?;

    // Read HandshakeAck response
    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)?;
    let resp_len = u32::from_le_bytes(len_buf) as usize;

    let mut resp_buf = vec![0u8; resp_len];
    file.read_exact(&mut resp_buf)?;

    let roundtrip_us = start_t.elapsed().as_micros();
    let ack_msg: IpcMessage = serde_json::from_slice(&resp_buf)?;

    println!("\n🟢 Status: ONLINE & HEALTHY");
    println!("   Pipe Path:          {}", pipe_name);
    println!("   IPC Connect Time:   {}µs", connect_us);
    println!("   Handshake RTT:      {}µs", roundtrip_us);

    if let IpcMessage::HandshakeAck { viewport } = ack_msg {
        println!("   Display Viewport:   {}x{} (scale: {:.2}x)", viewport.width, viewport.height, viewport.scale_factor);
    }
    println!("   Safety Kill-Switch: ARMED (Press F12 anytime to freeze I/O)\n");

    Ok(())
}
