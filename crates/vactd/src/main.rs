//! `vactd` — Vector Agent Context Transport Daemon
//!
//! Entry point, DXGI frame capture, GPU compute pipeline, WinRT OCR engine,
//! Vector Scene Graph DAG assembly, Named Pipe IPC, Safety Controls, and Overlay.
//!
//! # V14 Deliverables
//! - Native CLI commands: `vactd start`, `vactd status`, `vactd once`, `vactd bench`
//! - Global Emergency Safety Kill-Switch (Press `F12` anytime to freeze I/O)
//! - Native zero-overhead transparent Win32 screen overlay (`--overlay`)
//! - Structured logging and live terminal metrics dashboard

mod capture;
pub mod cli;
pub mod compute;
pub mod io_bus;
pub mod ipc;
pub mod ocr;
pub mod overlay;
pub mod safety;
pub mod taskbar;
pub mod os_metadata;
pub mod memory;

use std::{
    env,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use capture::{CapturedFrame, DxgiCapture, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT};
use cli::{parse_args, print_help, probe_status, Command};
use compute::{
    BenchmarkConfig, BilateralConfig, BilateralGradientPipeline, CclConfig, CclPipeline,
    GpuBenchmark, GpuContext,
};
use io_bus::IoBus;
use ipc::IpcServer;
use memory::MemoryStore;
use ocr::{OcrEngine, OcrTimings};
use os_metadata::OsMetadataProvider;
use overlay::{DebugOverlay, OverlayTelemetry};
use safety::SafetyMonitor;
use vact_core::{build_hierarchy, print_tree, BoundingBox};
use vact_protocol::diff::{FrameOutput, StateTracker};
use vact_protocol::ipc::IpcMessage;
use vact_protocol::scene::{build_scene_graph, print_scene_graph};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── 0. Set Per-Monitor V2 DPI Awareness for exact 1:1 hardware pixel coordinate alignment ──
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let args: Vec<String> = env::args().collect();
    let command = parse_args(&args);

    // ── 1. Handle Help and Diagnostic Commands ────────────────────────────────
    match command {
        Command::Help => {
            print_help();
            return Ok(());
        }
        Command::Status { pipe_name } => {
            return probe_status(&pipe_name);
        }
        _ => {}
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!(
        "🚀 vactd v{} starting (vact-core v{})",
        env!("CARGO_PKG_VERSION"),
        vact_core::VERSION,
    );

    // ── 2. Initialise GPU Compute Context ─────────────────────────────────────
    log::info!("Initialising GPU compute context (wgpu DX12/Vulkan/DX11)...");
    let gpu_ctx = GpuContext::new()?;
    log::info!(
        "GPU compute context ready: {} ({:?})",
        gpu_ctx.adapter_name(),
        gpu_ctx.backend()
    );

    // ── 3. Handle Standalone Benchmark Command ────────────────────────────────
    if let Command::Bench {
        iterations,
        test_readback,
    } = command
    {
        log::info!(
            "Running GPU compute benchmark ({} iterations, readback={})...",
            iterations,
            test_readback
        );
        let config = BenchmarkConfig {
            iterations,
            warmup_iterations: 10,
            width: 1920,
            height: 1080,
            test_readback,
        };
        let report = GpuBenchmark::run(&gpu_ctx, config)?;
        report.print_report();
        return Ok(());
    }

    // ── 4. Ctrl-C Handler ─────────────────────────────────────────────────────
    let running = Arc::new(AtomicBool::new(true));
    {
        let r = Arc::clone(&running);
        ctrlc::set_handler(move || {
            log::info!("Interrupt received — draining and shutting down.");
            r.store(false, Ordering::SeqCst);
        })?;
    }

    // ── 5. Unpack Options for Start / Once Modes ──────────────────────────────
    let (is_once, show_overlay, is_json_log) = match command {
        Command::Once { overlay } => (true, overlay, false),
        Command::Start {
            overlay, json_log, ..
        } => (false, overlay, json_log),
        _ => (false, false, false),
    };

    // ── 6. Initialise Spatial Memory Store, OS Metadata & I/O Bus ───────────
    let memory_store = match MemoryStore::open_default() {
        Ok(m) => {
            log::info!("Spatial-semantic agent memory store connected (vact_memory.db)");
            Some(Arc::new(m))
        }
        Err(e) => {
            log::warn!("Could not open persistent memory store: {e}");
            None
        }
    };
    let os_metadata = OsMetadataProvider::new();

    let io_bus = match &memory_store {
        Some(m) => Arc::new(IoBus::with_memory(Arc::clone(m))),
        None => Arc::new(IoBus::new()),
    };
    let _safety_monitor = SafetyMonitor::start(Arc::clone(&io_bus), Arc::clone(&running));

    // ── 7. Optional Native Screen Overlay ─────────────────────────────────────
    let _overlay = if show_overlay {
        Some(DebugOverlay::start(Arc::clone(&running)))
    } else {
        None
    };

    // ── 8. Initialise DXGI Output Duplication ─────────────────────────────────
    log::info!("Acquiring DXGI output duplication on primary monitor...");
    let mut capture = match DxgiCapture::new() {
        Ok(c) => c,
        Err(e) if e.code().0 == 0x80070005_u32 as i32 => {
            log::error!(
                "E_ACCESSDENIED from DuplicateOutput.\n\
                 \n\
                 DXGI Desktop Duplication requires the calling process to run on\n\
                 the interactive desktop (WinSta0\\Default).\n\
                 Fix: run inside standard interactive terminal.\n"
            );
            return Err(e.into());
        }
        Err(e) => return Err(e.into()),
    };
    log::info!("Capture ready on adapter: {}", capture.adapter_name());

    // ── 9. Initialise Perception Pipelines (Bilateral + CCL + WinRT OCR) ───────
    let mut bilateral_pipeline: Option<BilateralGradientPipeline> = None;
    let mut ccl_pipeline: Option<CclPipeline> = None;

    let bilateral_cfg = BilateralConfig {
        sigma_s: 2.5,
        sigma_r: 0.15,
        morph_r: 1,
    };
    let ccl_cfg = CclConfig::default();

    let ocr_engine = match OcrEngine::new() {
        Ok(engine) => {
            log::info!("WinRT OCR engine ready (language: {})", engine.language_tag());
            Some(engine)
        }
        Err(e) => {
            log::warn!("Could not initialize WinRT OCR engine: {e} (proceeding without OCR)");
            None
        }
    };

    // ── 10. Start Named Pipe Server (if daemon mode) ──────────────────────────
    let pipe_name = match &command {
        Command::Start { pipe_name, .. } => pipe_name.clone(),
        _ => r"\\.\pipe\VACT".to_string(),
    };
    let ipc_server = IpcServer::with_config(pipe_name, Some("127.0.0.1:4242"));
    if !is_once {
        ipc_server.start(Arc::clone(&io_bus));
    }

    let mut state_tracker = StateTracker::new();
    let mut frame_count: u64 = 0;
    let mut _timeout_count: u64 = 0;
    let mut saved_debug_artifacts = false;

    log::info!("⚡ VACT Perception Engine ACTIVE. Streaming 60 FPS state machine...");

    while running.load(Ordering::SeqCst) {
        match capture.capture_frame() {
            Ok(frame) => {
                // If initial captured frame is blank (DWM has not presented yet), retry
                let is_blank = frame.data.iter().step_by(512).all(|&b| b == 0);
                if is_blank && frame_count < 10 {
                    thread::sleep(Duration::from_millis(25));
                    continue;
                }

                frame_count += 1;

                // Lazily instantiate or resize pipelines to match frame dimensions
                let bil_pl = match bilateral_pipeline.as_mut() {
                    Some(p) => {
                        if p.width() != frame.width || p.height() != frame.height {
                            p.resize(frame.width, frame.height)?;
                        }
                        p
                    }
                    None => {
                        let p = BilateralGradientPipeline::new(
                            &gpu_ctx,
                            frame.width,
                            frame.height,
                            bilateral_cfg.clone(),
                        )?;
                        bilateral_pipeline = Some(p);
                        bilateral_pipeline.as_mut().unwrap()
                    }
                };

                let ccl_pl = match ccl_pipeline.as_mut() {
                    Some(p) => {
                        if p.width() != frame.width || p.height() != frame.height {
                            p.resize(frame.width, frame.height)?;
                        }
                        p
                    }
                    None => {
                        let p = CclPipeline::new(
                            &gpu_ctx,
                            frame.width,
                            frame.height,
                            ccl_cfg.clone(),
                        )?;
                        ccl_pipeline = Some(p);
                        ccl_pipeline.as_mut().unwrap()
                    }
                };

                // 1. Execute GPU Bilateral Saliency Passes
                let t_bil = bil_pl.execute(&frame.data)?;

                // 2. Read back universal RGBA8 heatmap & extract Bounding Boxes
                let heat_rgba = bil_pl.readback_heatmap()?;
                let (boxes, t_ccl) = ccl_pl.extract_from_heatmap_rgba(&heat_rgba);

                // 3. V6 — Build Geometric Containment Hierarchy
                let t_hier_start = std::time::Instant::now();
                let hierarchy = build_hierarchy(&boxes);
                let t_hier_us = t_hier_start.elapsed().as_micros() as u64;

                // 4. V7 — Hardware-accelerated WinRT OCR Text Extraction
                let (text_regions, t_ocr) = match &ocr_engine {
                    Some(ocr) => ocr.extract_all(&frame.data, frame.width, frame.height, &boxes, 64),
                    None => (Vec::new(), OcrTimings::default()),
                };

                // 5. V8 — Vector Scene Graph (DAG) Assembly
                let t_sg_start = std::time::Instant::now();
                let active_window = get_active_window_title();
                let mut scene_graph = build_scene_graph(
                    &hierarchy,
                    &text_regions,
                    frame.width,
                    frame.height,
                    frame_count,
                    active_window,
                );
                // Enrich untextured taskbar icon nodes with semantic application names
                taskbar::enrich_scene_graph_taskbar(&mut scene_graph);

                // Enrich unlabelled controls with OS accessibility (UIAutomation & Win32)
                os_metadata.enrich_scene_graph(&mut scene_graph);

                // Enrich nodes with photometric color classification and hex palettes
                vact_protocol::scene::enrich_scene_graph_colors(
                    &mut scene_graph,
                    &frame.data,
                    frame.width,
                    frame.height,
                );

                // Enrich nodes with perceptual hashes (pHash) and learned memory labels
                if let Some(ref mem) = memory_store {
                    let app_name = scene_graph.active_window.clone().unwrap_or_else(|| "*".to_string());
                    memory::enrich_scene_graph_memory(
                        &mut scene_graph,
                        &frame.data,
                        frame.width,
                        frame.height,
                        &app_name,
                        mem,
                    );
                }
                let t_sg_us = t_sg_start.elapsed().as_micros() as u64;

                // 6. Export debug artifacts on first frame or in once mode
                if !saved_debug_artifacts {
                    let _ = save_bgra_png(&frame, "debug_frame.png");
                    let _ = save_boxes_overlay_png(&frame, &boxes, "debug_boxes.png");

                    if is_once {
                        log::info!("── V8 Scene Graph Hierarchy ──");
                        print_tree(&hierarchy);
                        print_scene_graph(&scene_graph);

                        if let Ok(json_str) = serde_json::to_string_pretty(&scene_graph) {
                            println!("{}", json_str);
                            let _ = std::fs::write("debug_scene_graph.json", &json_str);
                        }
                    }
                    saved_debug_artifacts = true;
                }

                let node_count = count_nodes(&scene_graph.root);

                // 7. V9 — Process through Temporal Delta Diffing Engine
                let frame_output = state_tracker.process_frame(scene_graph);

                // Keep IoBus and IpcServer snapshot updated with the current stabilized scene graph
                if let Some(curr_graph) = state_tracker.current_scene() {
                    io_bus.update_scene(curr_graph.clone());
                    ipc_server.update_latest_snapshot(curr_graph.clone());
                }

                let (diff_bytes, diff_mutations) = match &frame_output {
                    FrameOutput::Snapshot(g) => {
                        let bytes = serde_json::to_string(g).unwrap_or_default().len();
                        ipc_server.broadcast(IpcMessage::Snapshot(g.clone()));
                        (bytes, 0)
                    }
                    FrameOutput::Diff(d) => {
                        let bytes = serde_json::to_string(d).unwrap_or_default().len();
                        ipc_server.broadcast(IpcMessage::Diff(d.clone()));
                        (bytes, d.mutations.len())
                    }
                };

                let total_us = frame.capture_us
                    + t_bil.total_us
                    + t_ccl.total_us
                    + t_hier_us
                    + t_ocr.total_us
                    + t_sg_us;

                let safety_status = if io_bus.is_emergency_stopped() {
                    "🚨 STOPPED"
                } else {
                    "🟢 ARMED"
                };

                // Update live Win32 Minecraft F3 scientific debug overlay if enabled
                if show_overlay {
                    if let Some(curr_graph) = state_tracker.current_scene() {
                        DebugOverlay::update_scene(curr_graph);
                    }
                    DebugOverlay::update_telemetry(OverlayTelemetry {
                        seq: frame_count,
                        fps: if total_us > 0 { (1_000_000.0 / total_us as f32).min(60.0) } else { 60.0 },
                        total_us,
                        dxgi_us: frame.capture_us,
                        bil_us: t_bil.total_us,
                        ccl_us: t_ccl.total_us,
                        ocr_us: t_ocr.total_us,
                        sg_us: t_sg_us + t_hier_us,
                        node_count,
                        diff_bytes,
                        diff_mutations,
                        active_window: get_active_window_title().unwrap_or_default(),
                        safety_status: safety_status.to_string(),
                        viewport_w: frame.width,
                        viewport_h: frame.height,
                    });
                }

                if is_json_log {
                    println!(
                        r#"{{"frame":{},"width":{},"height":{},"capture_us":{},"ccl_us":{},"ocr_us":{},"sg_us":{},"nodes":{},"diff_bytes":{},"mutations":{},"total_us":{},"safety":"{}"}}"#,
                        frame_count,
                        frame.width,
                        frame.height,
                        frame.capture_us,
                        t_ccl.total_us,
                        t_ocr.total_us,
                        t_sg_us,
                        node_count,
                        diff_bytes,
                        diff_mutations,
                        total_us,
                        safety_status,
                    );
                } else {
                    log::info!(
                        "frame {:>6}  {}×{}  │ dxgi={:>5}µs │ ccl={:>5}µs │ ocr={:>5}µs │ nodes={:>3} │ diff={:>4}B ({:>2} mut) │ safety={} │ total={:>5}µs",
                        frame_count,
                        frame.width,
                        frame.height,
                        frame.capture_us,
                        t_ccl.total_us,
                        t_ocr.total_us,
                        node_count,
                        diff_bytes,
                        diff_mutations,
                        safety_status,
                        total_us,
                    );
                }

                _timeout_count = 0;

                if is_once {
                    log::info!("once mode complete: 1 frame captured and exported.");
                    break;
                }
            }

            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                _timeout_count += 1;
                thread::sleep(Duration::from_millis(8));
            }

            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                log::warn!("DXGI_ERROR_ACCESS_LOST — reacquiring desktop duplication...");
                thread::sleep(Duration::from_millis(500));
                capture = DxgiCapture::new()?;
            }

            Err(e) => {
                log::error!("Unrecoverable capture error: {e}");
                return Err(e.into());
            }
        }
    }

    log::info!("vactd stopped cleanly — {} frames processed.", frame_count);
    Ok(())
}

fn save_bgra_png(frame: &CapturedFrame, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let rgba: Vec<u8> = frame
        .data
        .chunks_exact(4)
        .flat_map(|px| [px[2], px[1], px[0], 255])
        .collect();

    image::RgbaImage::from_raw(frame.width, frame.height, rgba)
        .ok_or("frame buffer dimensions don't match pixel count")?
        .save(path)?;

    Ok(())
}

fn save_boxes_overlay_png(
    frame: &CapturedFrame,
    boxes: &[BoundingBox],
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut rgba: Vec<u8> = frame
        .data
        .chunks_exact(4)
        .flat_map(|px| [px[2], px[1], px[0], 255])
        .collect();

    let w = frame.width as usize;
    let h = frame.height as usize;

    let colors: [[u8; 4]; 4] = [
        [0, 255, 128, 255],
        [0, 220, 255, 255],
        [255, 64, 192, 255],
        [255, 215, 0, 255],
    ];

    for (idx, b) in boxes.iter().enumerate() {
        let color = colors[idx % colors.len()];
        let x0 = b.x as usize;
        let y0 = b.y as usize;
        let x1 = (b.right().min(frame.width - 1)) as usize;
        let y1 = (b.bottom().min(frame.height - 1)) as usize;

        for border_y in [y0, y0.saturating_add(1), y1.saturating_sub(1), y1] {
            if border_y < h {
                for x in x0..=x1 {
                    if x < w {
                        let offset = (border_y * w + x) * 4;
                        rgba[offset..offset + 4].copy_from_slice(&color);
                    }
                }
            }
        }

        for border_x in [x0, x0.saturating_add(1), x1.saturating_sub(1), x1] {
            if border_x < w {
                for y in y0..=y1 {
                    if y < h {
                        let offset = (y * w + border_x) * 4;
                        rgba[offset..offset + 4].copy_from_slice(&color);
                    }
                }
            }
        }
    }

    image::RgbaImage::from_raw(frame.width, frame.height, rgba)
        .ok_or("frame buffer dimensions don't match pixel count")?
        .save(path)?;

    Ok(())
}

fn get_active_window_title() -> Option<String> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }

    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return None;
    }

    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

fn count_nodes(node: &vact_protocol::SceneNode) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}
