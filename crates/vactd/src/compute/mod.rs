//! GPU Compute Pipeline Infrastructure.
//!
//! Provides hardware-accelerated compute shaders, device/queue lifecycle management,
//! texture staging/interop, and performance benchmarking for real-time perception.
//!
//! ## V3 — Passthrough baseline
//! [`PassthroughPipeline`] establishes the wgpu substrate and benchmarking harness.
//!
//! ## V4 — Bilateral Gradient Saliency
//! [`BilateralGradientPipeline`] replaces the passthrough with the full four-pass
//! perception pipeline: bilateral filter → Sobel gradient → morphological opening.
//!
//! ## V5 — Watershed CCL & Bounding Box Extraction
//! [`CclPipeline`] segments regions and produces discrete [`vact_core::BoundingBox`]es.

pub mod benchmark;
pub mod bilateral;
pub mod ccl;
pub mod context;
pub mod passthrough;

pub use benchmark::{BenchmarkConfig, BenchmarkReport, GpuBenchmark, LatencyStats};
pub use bilateral::{BilateralConfig, BilateralGradientPipeline, BilateralTimings};
pub use ccl::{CclConfig, CclPipeline, CclTimings};
pub use context::GpuContext;
pub use passthrough::{ComputeTimings, PassthroughPipeline, PassthroughUniforms};
