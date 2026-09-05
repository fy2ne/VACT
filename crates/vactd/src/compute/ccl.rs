//! Connected Component Labeling (CCL) & Bounding Box Extraction — V5.
//!
//! Extracts rectangular bounding boxes `BoundingBox { id, x, y, width, height }`
//! for interactive UI elements (buttons, inputs, cards, text labels, icons, containers).

use std::time::Instant;
use vact_core::BoundingBox;
use wgpu::{
    util::DeviceExt, BindGroup, BindGroupLayout,
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
    Buffer, BufferBindingType, BufferDescriptor, BufferUsages,
    ComputePipeline, ComputePipelineDescriptor, Extent3d,
    PipelineCompilationOptions, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StorageTextureAccess,
    Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, TextureView, TextureViewDescriptor, TextureViewDimension,
    COPY_BYTES_PER_ROW_ALIGNMENT,
};

use super::context::GpuContext;

/// Configuration parameters for CCL and bounding box extraction.
#[derive(Clone, Debug)]
pub struct CclConfig {
    /// Gradient magnitude threshold for considering a pixel a boundary edge (default: 0.08).
    pub edge_threshold: f32,
    /// Minimum bounding box area in square pixels to filter out noise (default: 36).
    pub min_box_area: u64,
    /// Minimum box width/height in pixels (default: 4).
    pub min_box_dimension: u32,
    /// Maximum screen area coverage ratio to ignore the outer full-screen frame (default: 0.95).
    pub max_screen_coverage: f32,
}

impl Default for CclConfig {
    fn default() -> Self {
        Self {
            edge_threshold: 0.08,
            min_box_area: 36,
            min_box_dimension: 4,
            max_screen_coverage: 0.95,
        }
    }
}

/// Timing breakdown for the CCL stage.
#[derive(Copy, Clone, Debug, Default)]
pub struct CclTimings {
    /// GPU thresholding dispatch time in microseconds.
    pub gpu_threshold_us: u64,
    /// GPU to CPU readback time in microseconds.
    pub readback_us:      u64,
    /// Disjoint-set labeling and bounding box reduction time in microseconds.
    pub label_reduce_us:  u64,
    /// Total CCL stage duration in microseconds.
    pub total_us:         u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CclUniforms {
    width:          u32,
    height:         u32,
    edge_threshold: f32,
    _pad:           f32,
}

/// Pipeline for GPU binary mask creation and CPU bounding box extraction.
pub struct CclPipeline {
    ctx:               GpuContext,
    width:             u32,
    height:            u32,
    config:            CclConfig,
    #[allow(dead_code)]
    pipeline:          ComputePipeline,
    #[allow(dead_code)]
    bind_group_layout: BindGroupLayout,
    #[allow(dead_code)]
    bind_group:        Option<BindGroup>,
    uniform_buffer:    Buffer,
    mask_texture:      Texture,
    #[allow(dead_code)]
    mask_view:         TextureView,
    #[allow(dead_code)]
    staging_buffer:    Buffer,
    #[allow(dead_code)]
    padded_bpr:        u32,
}

impl CclPipeline {
    pub fn new(
        ctx: &GpuContext,
        width: u32,
        height: u32,
        config: CclConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = &ctx.device;

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label:  Some("ccl_threshold_shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/ccl_threshold.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label:   Some("ccl_bgl"),
            entries: &[
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
                BindGroupLayoutEntry {
                    binding:    2,
                    visibility: ShaderStages::COMPUTE,
                    ty:         BindingType::StorageTexture {
                        access:         StorageTextureAccess::WriteOnly,
                        format:         TextureFormat::R32Float,
                        view_dimension: TextureViewDimension::D2,
                    },
                    count:      None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                Some("ccl_pipeline_layout"),
            bind_group_layouts:   &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label:               Some("ccl_compute_pipeline"),
            layout:              Some(&pipeline_layout),
            module:              &shader,
            entry_point:         Some("main"),
            compilation_options: PipelineCompilationOptions::default(),
            cache:               None,
        });

        let uniforms = CclUniforms {
            width,
            height,
            edge_threshold: config.edge_threshold,
            _pad:           0.0,
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:    Some("ccl_uniform_buffer"),
            contents: bytemuck::bytes_of(&uniforms),
            usage:    BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let (mask_texture, mask_view, staging_buffer, padded_bpr) =
            Self::create_resources(device, width, height)?;

        Ok(Self {
            ctx: ctx.clone(),
            width,
            height,
            config,
            pipeline,
            bind_group_layout,
            bind_group: None,
            uniform_buffer,
            mask_texture,
            mask_view,
            staging_buffer,
            padded_bpr,
        })
    }

    fn create_resources(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<(Texture, TextureView, Buffer, u32), Box<dyn std::error::Error>> {
        let size = Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let mask_texture = device.create_texture(&TextureDescriptor {
            label:           Some("ccl_mask_texture"),
            size,
            mip_level_count: 1,
            sample_count:    1,
            dimension:       TextureDimension::D2,
            format:          TextureFormat::R32Float,
            usage:           TextureUsages::STORAGE_BINDING
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC,
            view_formats:    &[],
        });
        let mask_view = mask_texture.create_view(&TextureViewDescriptor::default());

        let align = COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bpr = ((width * 4) + align - 1) & !(align - 1);
        let staging_buffer = device.create_buffer(&BufferDescriptor {
            label:              Some("ccl_staging_buffer"),
            size:               (padded_bpr * height) as u64,
            usage:              BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok((mask_texture, mask_view, staging_buffer, padded_bpr))
    }

    /// Execute CCL directly from the RGBA heatmap slice (where alpha carries gradient magnitude).
    ///
    /// This is 100% universal across all GPUs and avoids driver-specific float readback quirks.
    pub fn extract_from_heatmap_rgba(
        &self,
        heatmap_rgba: &[u8],
    ) -> (Vec<BoundingBox>, CclTimings) {
        let t_start = Instant::now();
        let total_pixels = (self.width * self.height) as usize;
        let mut gradients = Vec::with_capacity(total_pixels);

        for chunk in heatmap_rgba.chunks_exact(4) {
            // Alpha channel contains the normalized gradient magnitude [0..255]
            gradients.push((chunk[3] as f32) / 255.0);
        }

        let boxes = self.extract_boxes_cpu(&gradients);
        let total_us = t_start.elapsed().as_micros() as u64;

        (
            boxes,
            CclTimings {
                gpu_threshold_us: 0,
                readback_us:      0,
                label_reduce_us:  total_us,
                total_us,
            },
        )
    }

    /// Execute CCL directly from raw gradient magnitude slice.
    pub fn extract_from_gradients(
        &self,
        gradients: &[f32],
    ) -> (Vec<BoundingBox>, CclTimings) {
        let t_start = Instant::now();
        let boxes = self.extract_boxes_cpu(gradients);
        let total_us = t_start.elapsed().as_micros() as u64;

        (
            boxes,
            CclTimings {
                gpu_threshold_us: 0,
                readback_us:      0,
                label_reduce_us:  total_us,
                total_us,
            },
        )
    }

    /// Robust multi-scale Connected Component Labeling for UI element extraction.
    fn extract_boxes_cpu(&self, mask: &[f32]) -> Vec<BoundingBox> {
        let w = self.width as usize;
        let h = self.height as usize;
        let total_pixels = w * h;

        if mask.len() < total_pixels {
            return Vec::new();
        }

        // 1. Region-based segmentation (Non-edge regions)
        let region_boxes = self.segment_connected_regions(mask, w, h, false);

        // 2. Affordance clustering (Text and Icon foreground clusters)
        let affordance_boxes = self.segment_connected_regions(mask, w, h, true);

        // 3. Merge and deduplicate
        let mut all_boxes = region_boxes;
        for ab in affordance_boxes {
            // Only add if not identical to an existing region box
            let is_duplicate = all_boxes.iter().any(|rb| rb.iou(&ab) > 0.90);
            if !is_duplicate {
                all_boxes.push(ab);
            }
        }

        // Sort by area (largest to smallest)
        all_boxes.sort_by(|a, b| b.area().cmp(&a.area()));

        // Assign sequential IDs
        for (i, b) in all_boxes.iter_mut().enumerate() {
            b.id = (i + 1) as u32;
        }

        all_boxes
    }

    fn segment_connected_regions(
        &self,
        mask: &[f32],
        w: usize,
        h: usize,
        is_foreground: bool,
    ) -> Vec<BoundingBox> {
        let total_pixels = w * h;
        let mut parent: Vec<u32> = Vec::with_capacity(32768);
        parent.push(0); // index 0 unused

        let mut bbox_acc: Vec<(u32, u32, u32, u32, u32)> = Vec::with_capacity(32768);
        bbox_acc.push((u32::MAX, u32::MAX, 0, 0, 0));

        let mut labels = vec![0u32; total_pixels];
        let mut next_label = 1u32;

        #[inline(always)]
        fn find_root(parent: &mut [u32], mut i: u32) -> u32 {
            let mut root = i;
            while parent[root as usize] != root {
                root = parent[root as usize];
            }
            while i != root {
                let next = parent[i as usize];
                parent[i as usize] = root;
                i = next;
            }
            root
        }

        #[inline(always)]
        fn union_roots(
            parent: &mut [u32],
            bbox_acc: &mut [(u32, u32, u32, u32, u32)],
            r1: u32,
            r2: u32,
        ) -> u32 {
            if r1 == r2 {
                return r1;
            }
            let (target, source) = if r1 < r2 { (r1, r2) } else { (r2, r1) };
            parent[source as usize] = target;

            let s = bbox_acc[source as usize];
            let t = &mut bbox_acc[target as usize];
            t.0 = t.0.min(s.0);
            t.1 = t.1.min(s.1);
            t.2 = t.2.max(s.2);
            t.3 = t.3.max(s.3);
            t.4 += s.4;

            target
        }

        let thresh = self.config.edge_threshold;

        for y in 0..h {
            let row_offset = y * w;
            for x in 0..w {
                let idx = row_offset + x;
                let val = mask[idx];

                // If is_foreground is true, we connect pixels with val >= thresh (text/icons)
                // If is_foreground is false, we connect pixels with val < thresh (containers)
                let active = if is_foreground {
                    val >= thresh
                } else {
                    val < thresh
                };

                if !active {
                    continue;
                }

                let left_label = if x > 0 { labels[idx - 1] } else { 0 };
                let top_label  = if y > 0 { labels[idx - w] } else { 0 };

                let assigned_label = match (left_label != 0, top_label != 0) {
                    (false, false) => {
                        let lbl = next_label;
                        next_label += 1;
                        parent.push(lbl);
                        bbox_acc.push((x as u32, y as u32, x as u32, y as u32, 1));
                        lbl
                    }
                    (true, false) => {
                        let r = find_root(&mut parent, left_label);
                        let b = &mut bbox_acc[r as usize];
                        b.0 = b.0.min(x as u32);
                        b.1 = b.1.min(y as u32);
                        b.2 = b.2.max(x as u32);
                        b.3 = b.3.max(y as u32);
                        b.4 += 1;
                        r
                    }
                    (false, true) => {
                        let r = find_root(&mut parent, top_label);
                        let b = &mut bbox_acc[r as usize];
                        b.0 = b.0.min(x as u32);
                        b.1 = b.1.min(y as u32);
                        b.2 = b.2.max(x as u32);
                        b.3 = b.3.max(y as u32);
                        b.4 += 1;
                        r
                    }
                    (true, true) => {
                        let r_left = find_root(&mut parent, left_label);
                        let r_top  = find_root(&mut parent, top_label);
                        let root = union_roots(&mut parent, &mut bbox_acc, r_left, r_top);
                        let b = &mut bbox_acc[root as usize];
                        b.0 = b.0.min(x as u32);
                        b.1 = b.1.min(y as u32);
                        b.2 = b.2.max(x as u32);
                        b.3 = b.3.max(y as u32);
                        b.4 += 1;
                        root
                    }
                };

                labels[idx] = assigned_label;
            }
        }

        let screen_area = (self.width as u64) * (self.height as u64);
        let mut boxes = Vec::new();

        for lbl in 1..next_label {
            if parent[lbl as usize] == lbl {
                let (min_x, min_y, max_x, max_y, pixel_count) = bbox_acc[lbl as usize];
                if min_x > max_x || min_y > max_y {
                    continue;
                }

                let width = max_x - min_x + 1;
                let height = max_y - min_y + 1;
                let area = (width as u64) * (height as u64);

                let min_area = if is_foreground { 16 } else { self.config.min_box_area };
                let min_dim  = if is_foreground { 3 }  else { self.config.min_box_dimension };
                let min_px   = if is_foreground { 8 }  else { 16 };

                if area < min_area || width < min_dim || height < min_dim || pixel_count < min_px {
                    continue;
                }

                if (area as f32 / screen_area as f32) >= self.config.max_screen_coverage {
                    continue;
                }

                boxes.push(BoundingBox::new(0, min_x, min_y, width, height));
            }
        }

        boxes
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
        if self.width == width && self.height == height {
            return Ok(());
        }

        self.width = width;
        self.height = height;

        let uniforms = CclUniforms {
            width,
            height,
            edge_threshold: self.config.edge_threshold,
            _pad:           0.0,
        };
        self.ctx
            .queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let (mask_texture, mask_view, staging_buffer, padded_bpr) =
            Self::create_resources(&self.ctx.device, width, height)?;

        self.mask_texture = mask_texture;
        self.mask_view = mask_view;
        self.staging_buffer = staging_buffer;
        self.padded_bpr = padded_bpr;
        self.bind_group = None;

        Ok(())
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}
