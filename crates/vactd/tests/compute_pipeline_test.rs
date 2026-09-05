use vactd::compute::{
    BenchmarkConfig, BilateralConfig, BilateralGradientPipeline, CclConfig, CclPipeline,
    GpuBenchmark, GpuContext, PassthroughPipeline,
};

#[test]
fn test_gpu_context_initialization() {
    let ctx = GpuContext::new().expect("Failed to initialize GPU compute context");
    assert!(!ctx.adapter_name().is_empty());
    println!("Active GPU Adapter: {} ({:?})", ctx.adapter_name(), ctx.backend());
}

#[test]
fn test_compute_pipeline_passthrough_execution() {
    let ctx = GpuContext::new().expect("Failed to initialize GPU compute context");

    let width = 64u32;
    let height = 64u32;
    let mut pipeline = PassthroughPipeline::new(&ctx, width, height)
        .expect("Failed to create PassthroughPipeline");

    // Construct synthetic BGRA8 frame with known test values
    let mut input_bgra = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            input_bgra[idx]     = 50;  // B
            input_bgra[idx + 1] = 100; // G
            input_bgra[idx + 2] = 200; // R
            input_bgra[idx + 3] = 255; // A
        }
    }

    // Execute compute pipeline
    let timings = pipeline
        .execute(&input_bgra)
        .expect("Compute pipeline execution failed");
    println!("Execution timings: {:?}", timings);

    // Read back output RGBA frame from GPU
    let output_rgba = pipeline
        .readback()
        .expect("Failed to read back frame from GPU");
    assert_eq!(output_rgba.len(), (width * height * 4) as usize);

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let r = output_rgba[idx];
            let g = output_rgba[idx + 1];
            let b = output_rgba[idx + 2];
            let a = output_rgba[idx + 3];

            assert_eq!(r, 200, "Red channel mismatch at ({}, {})", x, y);
            assert_eq!(g, 100, "Green channel mismatch at ({}, {})", x, y);
            assert_eq!(b, 50, "Blue channel mismatch at ({}, {})", x, y);
            assert_eq!(a, 255, "Alpha channel mismatch at ({}, {})", x, y);
        }
    }
}

#[test]
fn test_bilateral_and_ccl_bounding_box_extraction() {
    let ctx = GpuContext::new().expect("Failed to initialize GPU compute context");

    let width = 128u32;
    let height = 128u32;

    let mut bil_pipeline = BilateralGradientPipeline::new(
        &ctx,
        width,
        height,
        BilateralConfig::default(),
    ).expect("Failed to initialize BilateralGradientPipeline");

    let ccl_pipeline = CclPipeline::new(
        &ctx,
        width,
        height,
        CclConfig {
            edge_threshold: 0.08,
            min_box_area: 16,
            min_box_dimension: 4,
            max_screen_coverage: 0.95,
        },
    ).expect("Failed to initialize CclPipeline");

    // Draw a synthetic 30x20 button rectangle inside a dark background
    // Background: dark gray (30, 30, 30)
    // Button at (40, 40) size 40x30: light blue (200, 150, 50)
    let mut bgra = vec![30u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            bgra[idx + 3] = 255; // Alpha
            if x >= 40 && x < 80 && y >= 40 && y < 70 {
                bgra[idx]     = 200; // B
                bgra[idx + 1] = 150; // G
                bgra[idx + 2] = 50;  // R
            }
        }
    }

    let timings = bil_pipeline.execute(&bgra).expect("Bilateral execute failed");
    assert!(timings.total_us > 0);

    let heatmap = bil_pipeline.readback_heatmap().expect("Heatmap readback failed");
    assert_eq!(heatmap.len(), (width * height * 4) as usize);

    let (boxes, ccl_timings) = ccl_pipeline.extract_from_heatmap_rgba(&heatmap);
    println!("Detected {} UI bounding boxes in {:?} µs", boxes.len(), ccl_timings.total_us);

    // Should detect the button box around (40, 40)
    assert!(!boxes.is_empty(), "Should extract at least one bounding box for the button");
    let button = boxes.iter().find(|b| b.width >= 35 && b.height >= 25);
    assert!(button.is_some(), "Expected button bounding box found: {:?}", boxes);
}

#[test]
fn test_compute_benchmark_suite() {
    let ctx = GpuContext::new().expect("Failed to initialize GPU compute context");

    let config = BenchmarkConfig {
        iterations: 10,
        warmup_iterations: 2,
        width: 256,
        height: 256,
        test_readback: true,
    };

    let report = GpuBenchmark::run(&ctx, config).expect("Benchmark execution failed");
    assert_eq!(report.iterations, 10);
    assert_eq!(report.resolution, (256, 256));
    assert!(report.upload.mean_us > 0.0);
    assert!(report.dispatch.mean_us > 0.0);
    assert!(report.total.mean_us > 0.0);

    report.print_report();
}
