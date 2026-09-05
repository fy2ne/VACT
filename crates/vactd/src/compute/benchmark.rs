//! GPU Compute Pipeline Benchmarking Suite.
//!
//! Measures Host-to-Device transfer latency, compute dispatch execution time,
//! Device-to-Host readback latency, and overall perceptual frame throughput.

use std::time::Instant;

use super::{
    context::GpuContext,
    passthrough::{ComputeTimings, PassthroughPipeline},
};

/// Configuration parameters for running GPU compute benchmarks.
#[derive(Clone, Debug)]
pub struct BenchmarkConfig {
    pub iterations:        usize,
    pub warmup_iterations: usize,
    pub width:             u32,
    pub height:            u32,
    pub test_readback:     bool,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            iterations:        100,
            warmup_iterations: 10,
            width:             1920,
            height:            1080,
            test_readback:     true,
        }
    }
}

/// Aggregated statistical metrics for a benchmark timing distribution.
#[derive(Clone, Debug)]
pub struct LatencyStats {
    pub min_us:     u64,
    pub max_us:     u64,
    pub mean_us:    f64,
    pub p50_us:     u64,
    pub p95_us:     u64,
    pub p99_us:     u64,
    pub std_dev_us: f64,
}

impl LatencyStats {
    pub fn compute(mut values: Vec<u64>) -> Self {
        if values.is_empty() {
            return Self {
                min_us:     0,
                max_us:     0,
                mean_us:    0.0,
                p50_us:     0,
                p95_us:     0,
                p99_us:     0,
                std_dev_us: 0.0,
            };
        }

        values.sort_unstable();
        let n = values.len();
        let min_us = values[0];
        let max_us = values[n - 1];
        let sum: u64 = values.iter().sum();
        let mean_us = sum as f64 / n as f64;

        let p50_us = values[(n as f64 * 0.50) as usize];
        let p95_us = values[((n as f64 * 0.95) as usize).min(n - 1)];
        let p99_us = values[((n as f64 * 0.99) as usize).min(n - 1)];

        let variance = values
            .iter()
            .map(|&v| {
                let diff = v as f64 - mean_us;
                diff * diff
            })
            .sum::<f64>()
            / n as f64;
        let std_dev_us = variance.sqrt();

        Self {
            min_us,
            max_us,
            mean_us,
            p50_us,
            p95_us,
            p99_us,
            std_dev_us,
        }
    }
}

/// Comprehensive benchmark results report across all pipeline stages.
#[derive(Clone, Debug)]
pub struct BenchmarkReport {
    pub adapter_name:   String,
    pub backend:        String,
    pub resolution:     (u32, u32),
    pub frame_bytes:    usize,
    pub iterations:     usize,
    pub upload:         LatencyStats,
    pub dispatch:       LatencyStats,
    pub readback:       Option<LatencyStats>,
    pub total:          LatencyStats,
    pub throughput_fps: f64,
    pub throughput_gbs: f64,
}

impl BenchmarkReport {
    /// Formats the benchmark report to stdout / log with structured alignment.
    pub fn print_report(&self) {
        log::info!("╔════════════════════════════════════════════════════════════════════════════════════════╗");
        log::info!("║                          VACT V3 — GPU COMPUTE BENCHMARK REPORT                        ║");
        log::info!("╠════════════════════════════════════════════════════════════════════════════════════════╣");
        log::info!(
            "║ Adapter       : {:<71} ║",
            format!("{} ({})", self.adapter_name, self.backend)
        );
        log::info!(
            "║ Resolution    : {:<71} ║",
            format!("{}×{} ({:.2} MB/frame)", self.resolution.0, self.resolution.1, self.frame_bytes as f64 / (1024.0 * 1024.0))
        );
        log::info!("║ Iterations    : {:<71} ║", self.iterations);
        log::info!("╠════════════════════════════════════════════════════════════════════════════════════════╣");
        log::info!("║ STAGE              │    MIN µs │    AVG µs │    P50 µs │    P95 µs │    P99 µs │    MAX µs ║");
        log::info!("╟────────────────────┼───────────┼───────────┼───────────┼───────────┼───────────┼───────────╢");
        log::info!(
            "║ Host→GPU Upload    │ {:>9} │ {:>9.1} │ {:>9} │ {:>9} │ {:>9} │ {:>9} ║",
            self.upload.min_us, self.upload.mean_us, self.upload.p50_us, self.upload.p95_us, self.upload.p99_us, self.upload.max_us
        );
        log::info!(
            "║ Compute Dispatch   │ {:>9} │ {:>9.1} │ {:>9} │ {:>9} │ {:>9} │ {:>9} ║",
            self.dispatch.min_us, self.dispatch.mean_us, self.dispatch.p50_us, self.dispatch.p95_us, self.dispatch.p99_us, self.dispatch.max_us
        );
        if let Some(ref rb) = self.readback {
            log::info!(
                "║ GPU→Host Readback  │ {:>9} │ {:>9.1} │ {:>9} │ {:>9} │ {:>9} │ {:>9} ║",
                rb.min_us, rb.mean_us, rb.p50_us, rb.p95_us, rb.p99_us, rb.max_us
            );
        }
        log::info!("╟────────────────────┼───────────┼───────────┼───────────┼───────────┼───────────┼───────────╢");
        log::info!(
            "║ Total Pipeline     │ {:>9} │ {:>9.1} │ {:>9} │ {:>9} │ {:>9} │ {:>9} ║",
            self.total.min_us, self.total.mean_us, self.total.p50_us, self.total.p95_us, self.total.p99_us, self.total.max_us
        );
        log::info!("╠════════════════════════════════════════════════════════════════════════════════════════╣");
        log::info!(
            "║ Peak Throughput: {:>7.1} fps  |  VRAM Ingestion Bandwidth: {:>6.2} GB/s                 ║",
            self.throughput_fps, self.throughput_gbs
        );
        log::info!("╚════════════════════════════════════════════════════════════════════════════════════════╝");
    }
}

/// GPU Compute Benchmarking Engine.
pub struct GpuBenchmark;

impl GpuBenchmark {
    /// Execute benchmark suite measuring upload, dispatch, and readback latencies.
    pub fn run(
        ctx:    &GpuContext,
        config: BenchmarkConfig,
    ) -> Result<BenchmarkReport, Box<dyn std::error::Error>> {
        let width = config.width;
        let height = config.height;
        let frame_bytes = (width * height * 4) as usize;

        // Generate synthetic test frame (gradient pattern)
        let mut test_frame = vec![0u8; frame_bytes];
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                test_frame[idx]     = (x % 256) as u8;       // B
                test_frame[idx + 1] = (y % 256) as u8;       // G
                test_frame[idx + 2] = ((x + y) % 256) as u8; // R
                test_frame[idx + 3] = 255;                   // A
            }
        }

        let mut pipeline = PassthroughPipeline::new(ctx, width, height)?;

        // Warmup runs
        for _ in 0..config.warmup_iterations {
            let _ = pipeline.execute(&test_frame)?;
            if config.test_readback {
                let _ = pipeline.readback()?;
            }
        }

        let mut uploads   = Vec::with_capacity(config.iterations);
        let mut dispatches = Vec::with_capacity(config.iterations);
        let mut readbacks  = Vec::with_capacity(config.iterations);
        let mut totals     = Vec::with_capacity(config.iterations);

        for _ in 0..config.iterations {
            let t_iter_start = Instant::now();
            let timings: ComputeTimings = pipeline.execute(&test_frame)?;

            let rb_us = if config.test_readback {
                let t_rb_start = Instant::now();
                let _ = pipeline.readback()?;
                t_rb_start.elapsed().as_micros() as u64
            } else {
                0
            };

            let total_iter_us = t_iter_start.elapsed().as_micros() as u64;

            uploads.push(timings.upload_us);
            dispatches.push(timings.dispatch_us);
            if config.test_readback {
                readbacks.push(rb_us);
            }
            totals.push(total_iter_us);
        }

        let upload_stats   = LatencyStats::compute(uploads);
        let dispatch_stats = LatencyStats::compute(dispatches);
        let readback_stats = if config.test_readback {
            Some(LatencyStats::compute(readbacks))
        } else {
            None
        };
        let total_stats = LatencyStats::compute(totals);

        let avg_total_sec = total_stats.mean_us / 1_000_000.0;
        let throughput_fps = if avg_total_sec > 0.0 {
            1.0 / avg_total_sec
        } else {
            0.0
        };
        let throughput_gbs = (throughput_fps * frame_bytes as f64) / (1024.0 * 1024.0 * 1024.0);

        ctx.device.poll(wgpu::Maintain::Wait);

        Ok(BenchmarkReport {
            adapter_name:   ctx.adapter_name().to_string(),
            backend:        format!("{:?}", ctx.backend()),
            resolution:     (width, height),
            frame_bytes,
            iterations:     config.iterations,
            upload:         upload_stats,
            dispatch:       dispatch_stats,
            readback:       readback_stats,
            total:          total_stats,
            throughput_fps,
            throughput_gbs,
        })
    }
}
