//! Bilateral Gradient Saliency Pipeline — V4/V5 (Universal RGBA8 Architecture).
//!
//! Three-pass hardware-accelerated perception compute pipeline engineered for 100%
//! compatibility across all GPU architectures (Intel HD/UHD, AMD Radeon, NVIDIA GeForce/RTX,
//! Apple Silicon, Qualcomm Adreno, ARM Mali) on all graphics backends (DX11, DX12, Vulkan, OpenGL).

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

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BilateralUniforms {
    width:   u32,
    height:  u32,
    sigma_s: f32,
    sigma_r: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct MorphUniforms {
    width:   u32,
    height:  u32,
    morph_r: u32,
    _pad:    u32,
}

/// Configurable parameters for the bilateral gradient pipeline.
#[derive(Clone, Debug)]
pub struct BilateralConfig {
    /// Spatial kernel standard deviation in pixels (default: 2.2).
    pub sigma_s: f32,
    /// Radiometric/range kernel standard deviation in luma units (default: 0.15).
    pub sigma_r: f32,
    /// Morphological structuring element half-radius (default: 1 → 3×3 SE).
    pub morph_r: u32,
}

impl Default for BilateralConfig {
    fn default() -> Self {
        Self {
            sigma_s: 2.2,
            sigma_r: 0.15,
            morph_r: 1,
        }
    }
}

/// Per-frame timing breakdown for the bilateral gradient pipeline.
#[derive(Copy, Clone, Debug, Default)]
pub struct BilateralTimings {
    /// Host→Device texture upload (µs).
    pub upload_us:    u64,
    /// Pass 1: bilateral filter dispatch (µs).
    pub bilateral_us: u64,
    /// Pass 2: Sobel gradient dispatch (µs).
    pub sobel_us:     u64,
    /// Pass 3: morphological dilation dispatch (µs).
    pub morph_us:     u64,
    /// Total round-trip pipeline time (µs).
    pub total_us:     u64,
}

/// Manages all GPU resources for the bilateral gradient pipeline.
pub struct BilateralGradientPipeline {
    ctx:    GpuContext,
    width:  u32,
    height: u32,
    config: BilateralConfig,

    // Pipelines
    bilateral_pipeline: ComputePipeline,
    sobel_pipeline:     ComputePipeline,
    dilate_pipeline:    ComputePipeline,

    // Bind group layouts
    bilateral_bgl: BindGroupLayout,
    sobel_bgl:     BindGroupLayout,
    dilate_bgl:    BindGroupLayout,

    // Bind groups
    bilateral_bg: BindGroup,
    sobel_bg:     BindGroup,
    dilate_bg:    BindGroup,

    // Uniform buffers
    bilateral_ub: Buffer,
    morph_ub:     Buffer,

    // Textures (all using universal Rgba8Unorm format)
    input_texture: Texture,
    #[allow(dead_code)]
    input_view:    TextureView,
    luma_texture:  Texture,
    #[allow(dead_code)]
    luma_view:     TextureView,
    grad_texture:  Texture,
    #[allow(dead_code)]
    grad_view:     TextureView,
    final_texture: Texture,
    #[allow(dead_code)]
    final_view:    TextureView,
    heat_texture:  Texture,
    #[allow(dead_code)]
    heat_view:     TextureView,

    // Staging buffer for readback (universal Rgba8Unorm)
    heat_staging:   Buffer,
    padded_bpr_u8:  u32,
}

impl BilateralGradientPipeline {
    pub fn new(
        ctx:    &GpuContext,
        width:  u32,
        height: u32,
        config: BilateralConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = &ctx.device;

        let bilateral_shader = device.create_shader_module(ShaderModuleDescriptor {
            label:  Some("bilateral_filter_shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/bilateral_filter.wgsl").into()),
        });
        let sobel_shader = device.create_shader_module(ShaderModuleDescriptor {
            label:  Some("sobel_gradient_shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/sobel_gradient.wgsl").into()),
        });
        let dilate_shader = device.create_shader_module(ShaderModuleDescriptor {
            label:  Some("morph_dilate_shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/morph_dilate.wgsl").into()),
        });

        let bilateral_bgl = Self::make_bgl_3binding(device, "bilateral_bgl");
        let sobel_bgl     = Self::make_bgl_3binding(device, "sobel_bgl");
        let dilate_bgl    = Self::make_bgl_4binding(device, "dilate_bgl");

        let bilateral_pipeline = Self::make_pipeline(
            device, "bilateral_pipeline", &bilateral_shader, "main", &bilateral_bgl,
        );
        let sobel_pipeline = Self::make_pipeline(
            device, "sobel_pipeline", &sobel_shader, "main", &sobel_bgl,
        );
        let dilate_pipeline = Self::make_pipeline(
            device, "dilate_pipeline", &dilate_shader, "main", &dilate_bgl,
        );

        let bil_uniforms = BilateralUniforms {
            width, height,
            sigma_s: config.sigma_s,
            sigma_r: config.sigma_r,
        };
        let bilateral_ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("bilateral_uniforms"),
            contents: bytemuck::bytes_of(&bil_uniforms),
            usage:    BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let morph_uniforms = MorphUniforms {
            width, height, morph_r: config.morph_r, _pad: 0,
        };
        let morph_ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("morph_uniforms"),
            contents: bytemuck::bytes_of(&morph_uniforms),
            usage:    BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let (
            input_texture,  input_view,
            luma_texture,   luma_view,
            grad_texture,   grad_view,
            final_texture,  final_view,
            heat_texture,   heat_view,
            heat_staging,
            padded_bpr_u8,
        ) = Self::create_resources(device, width, height)?;

        let bilateral_bg = Self::build_3_bg(
            device, &bilateral_bgl, "bilateral_bg", &bilateral_ub, &input_view, &luma_view,
        );
        let sobel_bg = Self::build_3_bg(
            device, &sobel_bgl, "sobel_bg", &bilateral_ub, &luma_view, &grad_view,
        );
        let dilate_bg = Self::build_4_bg(
            device, &dilate_bgl, "dilate_bg", &morph_ub, &grad_view, &final_view, &heat_view,
        );

        Ok(Self {
            ctx: ctx.clone(),
            width, height, config,
            bilateral_pipeline, sobel_pipeline, dilate_pipeline,
            bilateral_bgl, sobel_bgl, dilate_bgl,
            bilateral_bg, sobel_bg, dilate_bg,
            bilateral_ub, morph_ub,
            input_texture, input_view,
            luma_texture,  luma_view,
            grad_texture,  grad_view,
            final_texture, final_view,
            heat_texture,  heat_view,
            heat_staging,
            padded_bpr_u8,
        })
    }

    /// Execute all GPU compute passes with hardware synchronization barriers.
    pub fn execute(
        &mut self,
        bgra_data: &[u8],
    ) -> Result<BilateralTimings, Box<dyn std::error::Error>> {
        let t_total = Instant::now();
        let device  = &self.ctx.device;
        let queue   = &self.ctx.queue;

        // 1. Upload Host Frame to GPU Input Texture
        let t_up = Instant::now();
        queue.write_texture(
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
            Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        let upload_us = t_up.elapsed().as_micros() as u64;

        let wg_x = (self.width  + 15) / 16;
        let wg_y = (self.height + 15) / 16;

        // Pass 1: Bilateral Filter
        let t_bil = Instant::now();
        {
            let mut enc = device.create_command_encoder(
                &CommandEncoderDescriptor { label: Some("bilateral_enc") },
            );
            {
                let mut pass = enc.begin_compute_pass(
                    &ComputePassDescriptor { label: Some("bilateral_pass"), timestamp_writes: None },
                );
                pass.set_pipeline(&self.bilateral_pipeline);
                pass.set_bind_group(0, &self.bilateral_bg, &[]);
                pass.dispatch_workgroups(wg_x, wg_y, 1);
            }
            queue.submit(Some(enc.finish()));
            device.poll(Maintain::Wait);
        }
        let bilateral_us = t_bil.elapsed().as_micros() as u64;

        // Pass 2: Sobel Gradient
        let t_sob = Instant::now();
        {
            let mut enc = device.create_command_encoder(
                &CommandEncoderDescriptor { label: Some("sobel_enc") },
            );
            {
                let mut pass = enc.begin_compute_pass(
                    &ComputePassDescriptor { label: Some("sobel_pass"), timestamp_writes: None },
                );
                pass.set_pipeline(&self.sobel_pipeline);
                pass.set_bind_group(0, &self.sobel_bg, &[]);
                pass.dispatch_workgroups(wg_x, wg_y, 1);
            }
            queue.submit(Some(enc.finish()));
            device.poll(Maintain::Wait);
        }
        let sobel_us = t_sob.elapsed().as_micros() as u64;

        // Pass 3: Morphological Edge Dilation & Heatmap
        let t_morph = Instant::now();
        {
            let mut enc = device.create_command_encoder(
                &CommandEncoderDescriptor { label: Some("morph_dilate_enc") },
            );
            {
                let mut pass = enc.begin_compute_pass(
                    &ComputePassDescriptor { label: Some("dilate_pass"), timestamp_writes: None },
                );
                pass.set_pipeline(&self.dilate_pipeline);
                pass.set_bind_group(0, &self.dilate_bg, &[]);
                pass.dispatch_workgroups(wg_x, wg_y, 1);
            }
            queue.submit(Some(enc.finish()));
            device.poll(Maintain::Wait);
        }
        let morph_us = t_morph.elapsed().as_micros() as u64;
        let total_us = t_total.elapsed().as_micros() as u64;

        Ok(BilateralTimings {
            upload_us,
            bilateral_us,
            sobel_us,
            morph_us,
            total_us,
        })
    }

    /// Read back the RGBA8 gradient heatmap from the GPU to CPU memory.
    pub fn readback_heatmap(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        self.readback_rgba(&self.heat_texture, &self.heat_staging, self.padded_bpr_u8)
    }

    /// Read back the normalized gradient magnitude map from the alpha channel of heat_texture.
    pub fn readback_gradient(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let rgba = self.readback_heatmap()?;
        let total_pixels = (self.width * self.height) as usize;
        let mut gradients = Vec::with_capacity(total_pixels);

        for chunk in rgba.chunks_exact(4) {
            gradients.push((chunk[3] as f32) / 255.0);
        }

        Ok(gradients)
    }

    /// Resize all GPU resources when desktop resolution changes.
    pub fn resize(
        &mut self,
        width:  u32,
        height: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.width == width && self.height == height {
            return Ok(());
        }
        self.width  = width;
        self.height = height;

        let bil_uniforms = BilateralUniforms {
            width, height,
            sigma_s: self.config.sigma_s,
            sigma_r: self.config.sigma_r,
        };
        self.ctx.queue.write_buffer(
            &self.bilateral_ub, 0, bytemuck::bytes_of(&bil_uniforms),
        );

        let morph_uniforms = MorphUniforms {
            width, height, morph_r: self.config.morph_r, _pad: 0,
        };
        self.ctx.queue.write_buffer(
            &self.morph_ub, 0, bytemuck::bytes_of(&morph_uniforms),
        );

        let (
            input_texture,  input_view,
            luma_texture,   luma_view,
            grad_texture,   grad_view,
            final_texture,  final_view,
            heat_texture,   heat_view,
            heat_staging,
            padded_bpr_u8,
        ) = Self::create_resources(&self.ctx.device, width, height)?;

        self.input_texture  = input_texture;
        self.input_view     = input_view;
        self.luma_texture   = luma_texture;
        self.luma_view      = luma_view;
        self.grad_texture   = grad_texture;
        self.grad_view      = grad_view;
        self.final_texture  = final_texture;
        self.final_view     = final_view;
        self.heat_texture   = heat_texture;
        self.heat_view      = heat_view;
        self.heat_staging   = heat_staging;
        self.padded_bpr_u8  = padded_bpr_u8;

        let device = &self.ctx.device;
        self.bilateral_bg = Self::build_3_bg(
            device, &self.bilateral_bgl, "bilateral_bg", &self.bilateral_ub,
            &self.input_view, &self.luma_view,
        );
        self.sobel_bg = Self::build_3_bg(
            device, &self.sobel_bgl, "sobel_bg", &self.bilateral_ub,
            &self.luma_view, &self.grad_view,
        );
        self.dilate_bg = Self::build_4_bg(
            device, &self.dilate_bgl, "dilate_bg", &self.morph_ub,
            &self.grad_view, &self.final_view, &self.heat_view,
        );

        Ok(())
    }

    pub fn width(&self)  -> u32 { self.width  }
    pub fn height(&self) -> u32 { self.height }
    pub fn final_view(&self) -> &TextureView { &self.final_view }

    fn create_resources(
        device: &wgpu::Device,
        width:  u32,
        height: u32,
    ) -> Result<(
        Texture, TextureView,
        Texture, TextureView,
        Texture, TextureView,
        Texture, TextureView,
        Texture, TextureView,
        Buffer,
        u32,
    ), Box<dyn std::error::Error>> {
        let size = Extent3d { width, height, depth_or_array_layers: 1 };

        let make_rgba8 = |label: &'static str, extra_usage: TextureUsages| {
            let tex = device.create_texture(&TextureDescriptor {
                label:           Some(label),
                size,
                mip_level_count: 1,
                sample_count:    1,
                dimension:       TextureDimension::D2,
                format:          TextureFormat::Rgba8Unorm,
                usage:           TextureUsages::STORAGE_BINDING
                    | TextureUsages::TEXTURE_BINDING
                    | extra_usage,
                view_formats:    &[],
            });
            let view = tex.create_view(&TextureViewDescriptor::default());
            (tex, view)
        };

        let input_texture = device.create_texture(&TextureDescriptor {
            label:           Some("bilateral_input_tex"),
            size,
            mip_level_count: 1,
            sample_count:    1,
            dimension:       TextureDimension::D2,
            format:          TextureFormat::Rgba8Unorm,
            usage:           TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        let input_view = input_texture.create_view(&TextureViewDescriptor::default());

        let (luma_texture,  luma_view)  = make_rgba8("bilateral_luma_tex",  TextureUsages::empty());
        let (grad_texture,  grad_view)  = make_rgba8("bilateral_grad_tex",  TextureUsages::empty());
        let (final_texture, final_view) = make_rgba8("bilateral_final_tex", TextureUsages::COPY_SRC);
        let (heat_texture,  heat_view)  = make_rgba8("bilateral_heat_tex",  TextureUsages::COPY_SRC);

        let align = COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bpr_u8 = ((width * 4) + align - 1) & !(align - 1);
        let heat_staging = device.create_buffer(&BufferDescriptor {
            label:              Some("bilateral_heat_staging"),
            size:               (padded_bpr_u8 * height) as u64,
            usage:              BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok((
            input_texture,  input_view,
            luma_texture,   luma_view,
            grad_texture,   grad_view,
            final_texture,  final_view,
            heat_texture,   heat_view,
            heat_staging,
            padded_bpr_u8,
        ))
    }

    fn make_bgl_3binding(device: &wgpu::Device, label: &'static str) -> BindGroupLayout {
        device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label:   Some(label),
            entries: &[
                uniform_entry(0),
                sampled_rgba8_entry(1),
                storage_rgba8_entry(2),
            ],
        })
    }

    fn make_bgl_4binding(device: &wgpu::Device, label: &'static str) -> BindGroupLayout {
        device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label:   Some(label),
            entries: &[
                uniform_entry(0),
                sampled_rgba8_entry(1),
                storage_rgba8_entry(2),
                storage_rgba8_entry(3),
            ],
        })
    }

    fn make_pipeline(
        device:  &wgpu::Device,
        label:   &str,
        module:  &wgpu::ShaderModule,
        entry:   &str,
        bgl:     &BindGroupLayout,
    ) -> ComputePipeline {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some(label),
            bind_group_layouts:   &[bgl],
            push_constant_ranges: &[],
        });
        device.create_compute_pipeline(&ComputePipelineDescriptor {
            label:        Some(label),
            layout:       Some(&layout),
            module,
            entry_point:  Some(entry),
            compilation_options: PipelineCompilationOptions::default(),
            cache:        None,
        })
    }

    fn build_3_bg(
        device:  &wgpu::Device,
        layout:  &BindGroupLayout,
        label:   &'static str,
        ub:      &Buffer,
        in_v:    &TextureView,
        out_v:   &TextureView,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label:   Some(label),
            layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: BindingResource::TextureView(in_v)  },
                BindGroupEntry { binding: 2, resource: BindingResource::TextureView(out_v) },
            ],
        })
    }

    fn build_4_bg(
        device:   &wgpu::Device,
        layout:   &BindGroupLayout,
        label:    &'static str,
        ub:       &Buffer,
        in_v:     &TextureView,
        out1_v:   &TextureView,
        out2_v:   &TextureView,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label:   Some(label),
            layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: BindingResource::TextureView(in_v)   },
                BindGroupEntry { binding: 2, resource: BindingResource::TextureView(out1_v) },
                BindGroupEntry { binding: 3, resource: BindingResource::TextureView(out2_v) },
            ],
        })
    }

    fn readback_rgba(
        &self,
        src_texture:  &Texture,
        staging:      &Buffer,
        padded_bpr:   u32,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let device = &self.ctx.device;
        let queue  = &self.ctx.queue;

        let mut enc = device.create_command_encoder(
            &CommandEncoderDescriptor { label: Some("rgba_readback_enc") },
        );
        enc.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture:   src_texture,
                mip_level: 0,
                origin:    Origin3d::ZERO,
                aspect:    TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: staging,
                layout: TexelCopyBufferLayout {
                    offset:         0,
                    bytes_per_row:  Some(padded_bpr),
                    rows_per_image: Some(self.height),
                },
            },
            Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        queue.submit(Some(enc.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = channel();
        slice.map_async(MapMode::Read, move |r| { let _ = tx.send(r); });
        device.poll(Maintain::Wait);
        rx.recv()??;

        let padded_data  = slice.get_mapped_range();
        let unpadded_row = (self.width * 4) as usize;
        let padded_row   = padded_bpr as usize;
        let mut tight    = vec![0u8; unpadded_row * self.height as usize];

        for row in 0..self.height as usize {
            let src = row * padded_row;
            let dst = row * unpadded_row;
            tight[dst..dst + unpadded_row]
                .copy_from_slice(&padded_data[src..src + unpadded_row]);
        }

        drop(padded_data);
        staging.unmap();

        Ok(tight)
    }
}

fn uniform_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty:         BindingType::Buffer {
            ty:                 BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size:   None,
        },
        count:      None,
    }
}

fn sampled_rgba8_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty:         BindingType::Texture {
            sample_type:    TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled:   false,
        },
        count:      None,
    }
}

fn storage_rgba8_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty:         BindingType::StorageTexture {
            access:         StorageTextureAccess::WriteOnly,
            format:         TextureFormat::Rgba8Unorm,
            view_dimension: TextureViewDimension::D2,
        },
        count:      None,
    }
}
