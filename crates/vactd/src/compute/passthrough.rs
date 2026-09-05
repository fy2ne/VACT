//! Passthrough GPU Compute Pipeline.
//!
//! Compiles and executes a WGSL compute shader on incoming desktop frames,
//! establishing the baseline substrate for subsequent image-processing passes
//! (Bilateral Gradient in V4, Watershed CCL in V5).

use std::{sync::mpsc::channel, time::Instant};
use wgpu::{
    util::DeviceExt, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType,
    Buffer, BufferBindingType, BufferDescriptor, BufferUsages, CommandEncoderDescriptor,
    ComputePassDescriptor, ComputePipeline, ComputePipelineDescriptor, Extent3d,
    Maintain, MapMode, Origin3d, PipelineCompilationOptions, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StorageTextureAccess, TexelCopyBufferInfo,
    TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, TextureView, TextureViewDescriptor, TextureViewDimension,
    COPY_BYTES_PER_ROW_ALIGNMENT,
};

use super::context::GpuContext;

/// Uniform buffer payload matching `passthrough.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PassthroughUniforms {
    pub width:    u32,
    pub height:   u32,
    pub flags:    u32,
    pub _padding: u32,
}

/// Latency breakdown across each stage of the GPU compute pass.
#[derive(Copy, Clone, Debug, Default)]
pub struct ComputeTimings {
    /// Host-to-Device texture upload duration in microseconds.
    pub upload_us:   u64,
    /// Compute shader workgroup dispatch & execution duration in microseconds.
    pub dispatch_us: u64,
    /// Device-to-Host readback duration (if requested) in microseconds.
    pub readback_us: u64,
    /// Total round-trip pipeline time in microseconds.
    pub total_us:    u64,
}

/// Manages the WGSL passthrough compute pipeline, textures, and execution buffers.
pub struct PassthroughPipeline {
    ctx:                  GpuContext,
    width:                u32,
    height:               u32,
    pipeline:             ComputePipeline,
    bind_group_layout:    BindGroupLayout,
    bind_group:           BindGroup,
    uniform_buffer:       Buffer,
    input_texture:        Texture,
    #[allow(dead_code)]
    input_view:           TextureView,
    output_texture:       Texture,
    #[allow(dead_code)]
    output_view:          TextureView,
    staging_buffer:       Buffer,
    padded_bytes_per_row: u32,
}

impl PassthroughPipeline {
    /// Construct a new `PassthroughPipeline` with pre-allocated textures for the specified resolution.
    pub fn new(ctx: &GpuContext, width: u32, height: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let device = &ctx.device;

        // ── 1. Compile WGSL Compute Shader ────────────────────────────────────
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label:  Some("passthrough_shader_module"),
            source: ShaderSource::Wgsl(include_str!("shaders/passthrough.wgsl").into()),
        });

        // ── 2. Create Bind Group Layout ───────────────────────────────────────
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label:   Some("passthrough_bind_group_layout"),
            entries: &[
                // Binding 0: Uniform buffer
                BindGroupLayoutEntry {
                    binding:    0,
                    visibility: ShaderStages::COMPUTE,
                    ty:         BindingType::Buffer {
                        ty:                 BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size:   None,
                    },
                    count:      None,
                },
                // Binding 1: Input texture (2D sampled/loadable)
                BindGroupLayoutEntry {
                    binding:    1,
                    visibility: ShaderStages::COMPUTE,
                    ty:         BindingType::Texture {
                        sample_type:    TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count:      None,
                },
                // Binding 2: Output storage texture (RGBA8 write-only)
                BindGroupLayoutEntry {
                    binding:    2,
                    visibility: ShaderStages::COMPUTE,
                    ty:         BindingType::StorageTexture {
                        access:         StorageTextureAccess::WriteOnly,
                        format:         TextureFormat::Rgba8Unorm,
                        view_dimension: TextureViewDimension::D2,
                    },
                    count:      None,
                },
            ],
        });

        // ── 3. Create Compute Pipeline ────────────────────────────────────────
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some("passthrough_pipeline_layout"),
            bind_group_layouts:   &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label:        Some("passthrough_compute_pipeline"),
            layout:       Some(&pipeline_layout),
            module:       &shader,
            entry_point:  Some("main"),
            compilation_options: PipelineCompilationOptions::default(),
            cache:        None,
        });

        // ── 4. Allocate Uniform Buffer ────────────────────────────────────────
        let uniforms = PassthroughUniforms {
            width,
            height,
            flags:    0,
            _padding: 0,
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("passthrough_uniform_buffer"),
            contents: bytemuck::bytes_of(&uniforms),
            usage:    BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        // ── 5. Allocate Textures and Buffers ──────────────────────────────────
        let (input_texture, input_view, output_texture, output_view, staging_buffer, padded_bytes_per_row) =
            Self::create_resources(device, width, height)?;

        // ── 6. Create Bind Group ──────────────────────────────────────────────
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label:   Some("passthrough_bind_group"),
            layout:  &bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding:  0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding:  1,
                    resource: BindingResource::TextureView(&input_view),
                },
                BindGroupEntry {
                    binding:  2,
                    resource: BindingResource::TextureView(&output_view),
                },
            ],
        });

        Ok(Self {
            ctx: ctx.clone(),
            width,
            height,
            pipeline,
            bind_group_layout,
            bind_group,
            uniform_buffer,
            input_texture,
            input_view,
            output_texture,
            output_view,
            staging_buffer,
            padded_bytes_per_row,
        })
    }

    /// Helper to allocate textures and staging buffer for given dimensions.
    fn create_resources(
        device: &wgpu::Device,
        width:  u32,
        height: u32,
    ) -> Result<(Texture, TextureView, Texture, TextureView, Buffer, u32), Box<dyn std::error::Error>> {
        let size = Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        // Input frame texture: BGRA8 matching DXGI output
        let input_texture = device.create_texture(&TextureDescriptor {
            label:           Some("passthrough_input_texture"),
            size,
            mip_level_count: 1,
            sample_count:    1,
            dimension:       TextureDimension::D2,
            format:          TextureFormat::Bgra8Unorm,
            usage:           TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        let input_view = input_texture.create_view(&TextureViewDescriptor::default());

        // Output frame texture: RGBA8 storage texture
        let output_texture = device.create_texture(&TextureDescriptor {
            label:           Some("passthrough_output_texture"),
            size,
            mip_level_count: 1,
            sample_count:    1,
            dimension:       TextureDimension::D2,
            format:          TextureFormat::Rgba8Unorm,
            usage:           TextureUsages::STORAGE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::TEXTURE_BINDING,
            view_formats:    &[],
        });
        let output_view = output_texture.create_view(&TextureViewDescriptor::default());

        // Staging buffer: padded row pitch to 256-byte boundary for readback
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) & !(align - 1);
        let buffer_size = (padded_bytes_per_row * height) as u64;

        let staging_buffer = device.create_buffer(&BufferDescriptor {
            label:              Some("passthrough_staging_buffer"),
            size:               buffer_size,
            usage:              BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok((
            input_texture,
            input_view,
            output_texture,
            output_view,
            staging_buffer,
            padded_bytes_per_row,
        ))
    }

    /// Reconfigure pipeline dimensions if desktop resolution changes.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
        if self.width == width && self.height == height {
            return Ok(());
        }

        self.width = width;
        self.height = height;

        let uniforms = PassthroughUniforms {
            width,
            height,
            flags:    0,
            _padding: 0,
        };
        self.ctx.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let (input_texture, input_view, output_texture, output_view, staging_buffer, padded_bytes_per_row) =
            Self::create_resources(&self.ctx.device, width, height)?;

        self.input_texture = input_texture;
        self.input_view = input_view;
        self.output_texture = output_texture;
        self.output_view = output_view;
        self.staging_buffer = staging_buffer;
        self.padded_bytes_per_row = padded_bytes_per_row;

        self.bind_group = self.ctx.device.create_bind_group(&BindGroupDescriptor {
            label:   Some("passthrough_bind_group"),
            layout:  &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding:  0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding:  1,
                    resource: BindingResource::TextureView(&self.input_view),
                },
                BindGroupEntry {
                    binding:  2,
                    resource: BindingResource::TextureView(&self.output_view),
                },
            ],
        });

        Ok(())
    }

    /// Execute the passthrough compute pass on the provided raw BGRA8 frame buffer.
    pub fn execute(&mut self, bgra_data: &[u8]) -> Result<ComputeTimings, Box<dyn std::error::Error>> {
        let t_start = Instant::now();

        // ── 1. Upload Host BGRA Data to GPU Texture ───────────────────────────
        let t_upload_start = Instant::now();
        self.ctx.queue.write_texture(
            TexelCopyTextureInfo {
                texture:   &self.input_texture,
                mip_level: 0,
                origin:    Origin3d::ZERO,
                aspect:    TextureAspect::All,
            },
            bgra_data,
            TexelCopyBufferLayout {
                offset:         0,
                bytes_per_row:  Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            Extent3d {
                width:                 self.width,
                height:                self.height,
                depth_or_array_layers: 1,
            },
        );
        let upload_us = t_upload_start.elapsed().as_micros() as u64;

        // ── 2. Encode and Dispatch Compute Pass ───────────────────────────────
        let t_dispatch_start = Instant::now();
        let mut encoder = self.ctx.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("passthrough_command_encoder"),
        });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label:            Some("passthrough_compute_pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &self.bind_group, &[]);

            // Calculate 16x16 workgroup dispatch grid dimensions
            let workgroups_x = (self.width + 15) / 16;
            let workgroups_y = (self.height + 15) / 16;
            compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        self.ctx.queue.submit(Some(encoder.finish()));
        self.ctx.device.poll(Maintain::Wait);
        let dispatch_us = t_dispatch_start.elapsed().as_micros() as u64;
        let total_us = t_start.elapsed().as_micros() as u64;

        Ok(ComputeTimings {
            upload_us,
            dispatch_us,
            readback_us: 0,
            total_us,
        })
    }

    /// Read back the processed RGBA8 frame from the GPU output storage texture to CPU memory.
    pub fn readback(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut encoder = self.ctx.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("passthrough_readback_encoder"),
        });

        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture:   &self.output_texture,
                mip_level: 0,
                origin:    Origin3d::ZERO,
                aspect:    TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &self.staging_buffer,
                layout: TexelCopyBufferLayout {
                    offset:         0,
                    bytes_per_row:  Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            Extent3d {
                width:                 self.width,
                height:                self.height,
                depth_or_array_layers: 1,
            },
        );

        self.ctx.queue.submit(Some(encoder.finish()));

        let buffer_slice = self.staging_buffer.slice(..);
        let (tx, rx) = channel();
        buffer_slice.map_async(MapMode::Read, move |res| {
            let _ = tx.send(res);
        });

        self.ctx.device.poll(Maintain::Wait);
        rx.recv()??;

        let padded_data = buffer_slice.get_mapped_range();
        let unpadded_row_size = (self.width * 4) as usize;
        let padded_row_size = self.padded_bytes_per_row as usize;
        let mut tight_data = vec![0u8; unpadded_row_size * self.height as usize];

        for row in 0..self.height as usize {
            let src_start = row * padded_row_size;
            let src_end = src_start + unpadded_row_size;
            let dst_start = row * unpadded_row_size;
            let dst_end = dst_start + unpadded_row_size;
            tight_data[dst_start..dst_end].copy_from_slice(&padded_data[src_start..src_end]);
        }

        drop(padded_data);
        self.staging_buffer.unmap();

        Ok(tight_data)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}
