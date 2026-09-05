//! GPU Context initialization and resource management via `wgpu`.
//!
//! Provides hardware-accelerated device and queue primitives targeting DirectX 12 / Vulkan / DX11
//! on Windows with deterministic latency characteristics.
//!
//! Adapter selection enumerates all backends (`Backends::all()`) and prioritizes real hardware
//! devices (DiscreteGpu or IntegratedGpu) across DX12, Vulkan, and DX11, bypassing software
//! emulators like Microsoft Basic Render Driver / llvmpipe.

use std::sync::Arc;
use wgpu::{
    Adapter, AdapterInfo, Backends, Device, DeviceDescriptor, DeviceType, Features, Instance,
    InstanceDescriptor, Limits, PowerPreference, Queue, RequestAdapterOptions,
};

/// Encapsulates the GPU device, queue, adapter, and instance.
#[derive(Clone)]
pub struct GpuContext {
    pub instance: Arc<Instance>,
    pub adapter:  Arc<Adapter>,
    pub device:   Arc<Device>,
    pub queue:    Arc<Queue>,
    pub info:     AdapterInfo,
}

impl GpuContext {
    /// Initialize a new GPU compute context.
    ///
    /// Requests a high-performance hardware adapter using any available backend.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        pollster::block_on(Self::new_async())
    }

    /// Asynchronous initializer for `GpuContext`.
    pub async fn new_async() -> Result<Self, Box<dyn std::error::Error>> {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });

        // Enumerate all adapters across all backends (DX12, DX11, Vulkan, GL)
        let all_adapters = instance.enumerate_adapters(Backends::all());
        log::info!("Discovered {} GPU compute adapters:", all_adapters.len());
        for a in &all_adapters {
            let i = a.get_info();
            log::info!("  • [{:?}] {} (Type: {:?}, Driver: {})", i.backend, i.name, i.device_type, i.driver);
        }

        // 1. Prefer Discrete GPU
        // 2. Prefer Integrated GPU
        // 3. Fallback to any non-CPU adapter
        // 4. Ultimate fallback to default adapter
        let selected_adapter = all_adapters.iter().find(|a| a.get_info().device_type == DeviceType::DiscreteGpu)
            .or_else(|| all_adapters.iter().find(|a| a.get_info().device_type == DeviceType::IntegratedGpu))
            .or_else(|| all_adapters.iter().find(|a| {
                let name = a.get_info().name.to_lowercase();
                !name.contains("basic render") && !name.contains("software") && a.get_info().device_type != DeviceType::Cpu
            }))
            .cloned();

        let adapter = match selected_adapter {
            Some(a) => {
                log::info!("Selected primary hardware GPU adapter: {} ({:?})", a.get_info().name, a.get_info().backend);
                a
            }
            None => {
                log::warn!(
                    "No dedicated or integrated hardware GPU found — requesting default fallback adapter. \
                     Note: compute shaders may run under CPU emulation."
                );
                instance
                    .request_adapter(&RequestAdapterOptions {
                        power_preference:       PowerPreference::HighPerformance,
                        compatible_surface:     None,
                        force_fallback_adapter: false,
                    })
                    .await
                    .ok_or("Failed to find any GPU adapter for compute pipeline")?
            }
        };

        let info = adapter.get_info();
        log::info!(
            "Active GPU Compute Adapter: {} (Backend: {:?}, Type: {:?})",
            info.name,
            info.backend,
            info.device_type
        );

        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label:             Some("vact_compute_device"),
                    required_features: Features::empty(),
                    required_limits:   Limits::downlevel_defaults().using_resolution(adapter.limits()),
                    memory_hints:      Default::default(),
                },
                None,
            )
            .await?;

        Ok(Self {
            instance: Arc::new(instance),
            adapter:  Arc::new(adapter),
            device:   Arc::new(device),
            queue:    Arc::new(queue),
            info,
        })
    }

    /// Returns the human-readable name of the active GPU adapter.
    pub fn adapter_name(&self) -> &str {
        &self.info.name
    }

    /// Returns the graphics/compute backend in use (e.g. Dx12, Vulkan, Dx11).
    pub fn backend(&self) -> wgpu::Backend {
        self.info.backend
    }
}
