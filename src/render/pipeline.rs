//! WGPU setup and drawing.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use winit::window::Window;

use super::camera::{Camera, HeightField};
use super::instances::UnitInstanceRaw;

const TERRAIN_GRID_SIZE: u32 = 512;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const BACKGROUND: wgpu::Color = wgpu::Color { r: 0.015, g: 0.02, b: 0.035, a: 1.0 };

/// Camera distances where cubes begin and finish collapsing into dots. A 2-unit cube
/// is about 8 pixels across at the start, and about 4 at the end, in a 1080p window.
const LOD_START: f32 = 330.0;
const LOD_END: f32 = 660.0;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    viewport_lod: [f32; 4],
    terrain_origin: [f32; 4],
    terrain_grid: [i32; 4],
    selection_box: [f32; 4],
    terrain_scale: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct CubeVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

fn cube_mesh() -> (Vec<CubeVertex>, Vec<u16>) {
    // Each face: normal, then two axes that span it.
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, u, v) in faces {
        let base = vertices.len() as u16;
        for (su, sv) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
            let position = std::array::from_fn(|axis| normal[axis] * 0.5 + u[axis] * su + v[axis] * sv);
            vertices.push(CubeVertex { position, normal });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

fn terrain_indices() -> Vec<u32> {
    let size = TERRAIN_GRID_SIZE;
    let mut indices = Vec::with_capacity(((size - 1) * (size - 1) * 6) as usize);
    for y in 0..size - 1 {
        for x in 0..size - 1 {
            let (a, b) = (y * size + x, y * size + x + 1);
            let (c, d) = ((y + 1) * size + x, (y + 1) * size + x + 1);
            indices.extend_from_slice(&[a, b, c, c, b, d]);
        }
    }
    indices
}

/// A growable instance buffer.
struct InstanceBuffer {
    buffer: wgpu::Buffer,
    capacity: usize,
    count: u32,
}

impl InstanceBuffer {
    fn new(device: &wgpu::Device, capacity: usize) -> Self {
        Self { buffer: Self::create(device, capacity), capacity, count: 0 }
    }

    fn create(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (capacity * std::mem::size_of::<UnitInstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, instances: &[UnitInstanceRaw]) {
        if instances.len() > self.capacity {
            self.capacity = instances.len().next_power_of_two();
            self.buffer = Self::create(device, self.capacity);
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(instances));
        self.count = instances.len() as u32;
    }
}

/// Everything drawn in one frame.
pub struct FrameScene<'a> {
    pub camera: &'a Camera,
    pub units: &'a [UnitInstanceRaw],
    pub projectiles: &'a [UnitInstanceRaw],
    pub obstacles: &'a [UnitInstanceRaw],
    /// The selection box in pixels, as `(min, max)`.
    pub selection_box: Option<([f64; 2], [f64; 2])>,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    unit_pipeline: wgpu::RenderPipeline,
    terrain_pipeline: wgpu::RenderPipeline,
    box_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,
    cube_vertices: wgpu::Buffer,
    cube_indices: wgpu::Buffer,
    cube_index_count: u32,
    terrain_index_buffer: wgpu::Buffer,
    terrain_index_count: u32,
    unit_buffer: InstanceBuffer,
    projectile_buffer: InstanceBuffer,
    obstacle_buffer: InstanceBuffer,
    samples_per_side: i32,
    cell_units: f64,
    height_scale: f32,
    icon_scale: f32,
    draw_box: bool,
    last_acquire: Duration,
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Renderer {
    pub fn new(window: Arc<Window>, heights: &HeightField) -> Self {
        let size = window.inner_size();
        let icon_scale = window.scale_factor() as f32;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance.create_surface(window).expect("window supports a surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("a graphics adapter is available");
        log::info!("adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ))
        .expect("the graphics device is available");

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // The height map, read directly in the vertex shader.
        let samples = heights.samples() as u32;
        let height_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("heights"),
            size: wgpu::Extent3d { width: samples, height: samples, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Sint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &height_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(heights.heights()),
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(samples * 2), rows_per_image: Some(samples) },
            wgpu::Extent3d { width: samples, height: samples, depth_or_array_layers: 1 },
        );
        let height_view = height_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Sint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&height_view) },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders.wgsl"));

        let depth = |write: bool, compare| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: write,
                depth_compare: compare,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        let targets = [Some(wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];

        let cube_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let instance_attributes = wgpu::vertex_attr_array![
            2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4
        ];
        let unit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("units"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_unit",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<CubeVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &cube_attributes,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<UnitInstanceRaw>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attributes,
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_unit",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(true, wgpu::CompareFunction::Greater),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_terrain",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(true, wgpu::CompareFunction::Greater),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let box_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("selection box"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_box",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_box",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::Always),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let (vertices, indices) = cube_mesh();
        let cube_vertices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("cube vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let cube_indices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("cube indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let grid_indices = terrain_indices();
        let terrain_index_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("terrain indices"),
            contents: bytemuck::cast_slice(&grid_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let depth_view = create_depth_view(&device, config.width, config.height);
        let unit_buffer = InstanceBuffer::new(&device, 8_192);
        let projectile_buffer = InstanceBuffer::new(&device, 8_192);
        let obstacle_buffer = InstanceBuffer::new(&device, 64);
        Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            unit_pipeline,
            terrain_pipeline,
            box_pipeline,
            bind_group,
            globals_buffer,
            cube_vertices,
            cube_indices,
            cube_index_count: indices.len() as u32,
            terrain_index_buffer,
            terrain_index_count: grid_indices.len() as u32,
            unit_buffer,
            projectile_buffer,
            obstacle_buffer,
            samples_per_side: heights.samples() as i32,
            cell_units: heights.cell_units(),
            height_scale: heights.units_per_step() as f32,
            icon_scale,
            draw_box: false,
            last_acquire: Duration::ZERO,
        }
    }

    /// How long the last frame waited for a drawable, which is how vsync paces frames.
    pub const fn last_acquire(&self) -> Duration {
        self.last_acquire
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, width, height);
    }

    fn prepare(&mut self, scene: &FrameScene<'_>) {
        let camera = scene.camera;
        let eye = camera.eye();
        let viewport = camera.viewport();

        // The grid's spacing doubles with altitude, and its first sample snaps to that
        // spacing so that hills do not shimmer as the camera moves.
        let cells = f64::from(TERRAIN_GRID_SIZE - 1);
        let wanted_reach = camera.altitude() * 16.0;
        let wanted_step = (wanted_reach / (cells * self.cell_units)).ceil().max(1.0) as i32;
        let step = (wanted_step as u32).next_power_of_two().min((self.samples_per_side - 1).max(1) as u32) as i32;
        let focus = camera.focus();
        let half_span = (TERRAIN_GRID_SIZE as i32 / 2) * step;
        let center = |value: f64| (value / self.cell_units) as i32;
        let snapped = |value: i32| (value - half_span).div_euclid(step) * step;
        let first = [snapped(center(focus[0])), snapped(center(focus[1]))];

        let selection_box = scene.selection_box.map_or([0.0; 4], |(min, max)| {
            let x = |value: f64| (value / viewport[0] * 2.0 - 1.0) as f32;
            let y = |value: f64| (1.0 - value / viewport[1] * 2.0) as f32;
            [x(min[0]), y(max[1]), x(max[0]), y(min[1])]
        });
        self.draw_box = scene.selection_box.is_some();

        let globals = Globals {
            view_proj: camera.view_projection().to_cols_array_2d(),
            viewport_lod: [viewport[0] as f32, viewport[1] as f32, LOD_START, LOD_END],
            terrain_origin: [
                (f64::from(first[0]) * self.cell_units - eye[0]) as f32,
                (f64::from(first[1]) * self.cell_units - eye[1]) as f32,
                eye[2] as f32,
                self.cell_units as f32,
            ],
            terrain_grid: [first[0], first[1], step, self.samples_per_side],
            selection_box,
            terrain_scale: [self.height_scale, self.icon_scale, 0.0, 0.0],
        };
        self.queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        self.unit_buffer.write(&self.device, &self.queue, scene.units);
        self.projectile_buffer.write(&self.device, &self.queue, scene.projectiles);
        self.obstacle_buffer.write(&self.device, &self.queue, scene.obstacles);
    }

    fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, depth: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(BACKGROUND), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                // Reversed Z: the far plane is 0.
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);

        pass.set_pipeline(&self.terrain_pipeline);
        pass.set_index_buffer(self.terrain_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.terrain_index_count, 0, 0..1);

        pass.set_pipeline(&self.unit_pipeline);
        pass.set_vertex_buffer(0, self.cube_vertices.slice(..));
        pass.set_index_buffer(self.cube_indices.slice(..), wgpu::IndexFormat::Uint16);
        for buffer in [&self.obstacle_buffer, &self.unit_buffer, &self.projectile_buffer] {
            if buffer.count > 0 {
                pass.set_vertex_buffer(1, buffer.buffer.slice(..));
                pass.draw_indexed(0..self.cube_index_count, 0, 0..buffer.count);
            }
        }

        if self.draw_box {
            pass.set_pipeline(&self.box_pipeline);
            pass.draw(0..24, 0..1);
        }
    }

    pub fn render(&mut self, scene: &FrameScene<'_>) {
        self.prepare(scene);
        let acquire_started = Instant::now();
        let acquired = self.surface.get_current_texture();
        self.last_acquire = acquire_started.elapsed();
        let frame = match acquired {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(error) => {
                log::warn!("skipping frame: {error}");
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        self.encode(&mut encoder, &view, &self.depth_view);
        self.queue.submit([encoder.finish()]);
        frame.present();
    }

    /// Draws a scene offscreen and saves it as a 24-bit BMP, for test runs.
    pub fn capture(&mut self, scene: &FrameScene<'_>, path: &Path) -> io::Result<()> {
        self.prepare(scene);
        let (width, height) = (self.config.width, self.config.height);
        let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: u64::from(bytes_per_row) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = create_depth_view(&self.device, width, height);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("capture") });
        self.encode(&mut encoder, &view, &depth);
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(height) },
            },
            extent,
        );
        self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("capture buffer maps"));
        self.device.poll(wgpu::Maintain::Wait);
        let swap_channels = matches!(self.config.format, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb);
        let bmp = encode_bmp(&slice.get_mapped_range(), width, height, bytes_per_row, swap_channels);
        readback.unmap();
        fs::write(path, bmp)
    }
}

/// Encodes 4-byte pixels as a top-down 24-bit BMP. BMP stores blue first.
fn encode_bmp(pixels: &[u8], width: u32, height: u32, stride: u32, rgba_input: bool) -> Vec<u8> {
    let row_size = (width * 3).next_multiple_of(4);
    let image_size = row_size * height;
    let mut out = Vec::with_capacity(54 + image_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + image_size).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(-(height as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&image_size.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for y in 0..height {
        let row = &pixels[(y * stride) as usize..];
        for x in 0..width as usize {
            let pixel = &row[x * 4..x * 4 + 4];
            if rgba_input {
                out.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            } else {
                out.extend_from_slice(&pixel[..3]);
            }
        }
        out.resize(out.len() + (row_size - width * 3) as usize, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_has_outward_facing_triangles() {
        let (vertices, indices) = cube_mesh();
        assert_eq!((vertices.len(), indices.len()), (24, 36));
        for triangle in indices.chunks(3) {
            let at = |i: usize| glam::Vec3::from(vertices[triangle[i] as usize].position);
            let face_normal = (at(1) - at(0)).cross(at(2) - at(0));
            let stored = glam::Vec3::from(vertices[triangle[0] as usize].normal);
            assert!(face_normal.dot(stored) > 0.0, "triangle {triangle:?} faces inward");
        }
    }

    #[test]
    fn terrain_grid_indices_stay_in_range() {
        let indices = terrain_indices();
        let vertices = TERRAIN_GRID_SIZE * TERRAIN_GRID_SIZE;
        assert!(indices.iter().all(|&index| index < vertices));
    }

    #[test]
    fn bmp_is_top_down_with_padded_rows() {
        // Two 1 x 1 rows of BGRA input with a 4-byte stride.
        let bmp = encode_bmp(&[1, 2, 3, 255, 4, 5, 6, 255], 1, 2, 4, false);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 54 + 8);
        assert_eq!(&bmp[54..58], &[1, 2, 3, 0]);
        assert_eq!(&bmp[58..62], &[4, 5, 6, 0]);
    }
}
