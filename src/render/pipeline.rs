//! WGPU setup and drawing.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use winit::window::Window;

use super::camera::{Camera, HeightField};
use super::instances::{InstanceBatches, UnitInstanceRaw};
use super::meshes::{MeshSet, MeshVertex, Shape};
use super::gpu_timing::GpuTimer;
use super::detail::detail_levels;
use super::land::LandField;
use super::shore::ShoreField;
use super::tiles::{tiled_indices, visible_tiles, GridPlacement, HeightRanges, TileRuns};
use super::waves::wave_levels;
use super::sight::{Presence, Sight};

/// The most terrain samples a side the grid holds: enough for a 20 km map at full detail.
const MAX_TERRAIN_GRID: u32 = 1025;
/// Samples per pixel, for smooth edges, where the graphics adapter supports it.
const MSAA_SAMPLES: u32 = 4;
/// Off the map: near black, so that the map stands out like a lit table.
const BACKGROUND: wgpu::Color = wgpu::Color { r: 0.006, g: 0.008, b: 0.011, a: 1.0 };

/// How strongly what is in sight is washed pale against the darker land out of sight: a third as
/// strong up close, all of it from halfway out, where the vision bubbles are what the map is read by.
pub fn strategic_share(zoom_from_closest: f64) -> f32 {
    let t = ((zoom_from_closest - 0.28) / 0.22).clamp(0.0, 1.0);
    (0.35 + 0.65 * t * t * (3.0 - 2.0 * t)) as f32
}

/// The size-to-distance ratios where a unit is fully a dot, and fully a cube. Based on
/// apparent size, so that large units stay solid for longer. About 4 and 8 pixels at 1080p.
pub(super) const LOD_DOT_RATIO: f32 = 0.003;
const LOD_CUBE_RATIO: f32 = 0.006;

/// Whether a part is small enough on screen to have begun turning into its dot, as the shader
/// works out (`dot_morph`): from there on its far model is drawn in its place.
fn is_small(part: &UnitInstanceRaw) -> bool {
    let length = |[x, y, z, _]: [f32; 4]| (x * x + y * y + z * z).sqrt();
    part.color[3] > 0.0 && length(part.model_matrix[0]) / length(part.model_matrix[3]).max(0.001) <= LOD_CUBE_RATIO
}
pub const FOG_CELL_UNITS: f64 = 80.0;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    viewport_lod: [f32; 4],
    terrain_origin: [f32; 4],
    terrain_grid: [i32; 4],
    selection_box: [f32; 4],
    terrain_scale: [f32; 4],
    /// The simulation's time in seconds, for waves, the terrain grid's samples a side, how
    /// strongly sight is washed pale, then the quality: 0 for `Low`, 1 for `High`.
    water: [f32; 4],
}

/// How much the land and sea shaders draw. `Low` is for weak graphics, such as a laptop's
/// built-in one in the browser, and `High` for the rest. Both draw the same picture until the
/// shaders give `Low` cheaper paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Low,
    High,
}

impl Quality {
    /// By name, for --quality and ?quality=.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// `Low` in the browser, which must run on any laptop, and `High` elsewhere.
    pub const fn default_for_platform() -> Self {
        if cfg!(target_arch = "wasm32") {
            Self::Low
        } else {
            Self::High
        }
    }

    const fn shader_value(self) -> f32 {
        match self {
            Self::Low => 0.0,
            Self::High => 1.0,
        }
    }
}

/// A growable instance buffer.
struct InstanceBuffer {
    buffer: wgpu::Buffer,
    capacity: usize,
}

impl InstanceBuffer {
    fn new(device: &wgpu::Device, capacity: usize) -> Self {
        Self { buffer: Self::create(device, capacity), capacity }
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
    }
}

/// The interface for one frame, ready to draw over the scene.
pub struct UiFrame {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub screen: egui_wgpu::ScreenDescriptor,
}

/// Everything drawn in one frame.
pub struct FrameScene<'a> {
    pub camera: &'a Camera,
    pub instances: &'a InstanceBatches,
    /// The selection box in pixels, as `(min, max)`.
    pub selection_box: Option<([f64; 2], [f64; 2])>,
    /// Seconds of wall time since the game started, which moves the waves.
    pub time: f32,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Depth, with a stencil that lets each pixel be shadowed only once.
    depth_format: wgpu::TextureFormat,
    samples: u32,
    targets: SceneTargets,
    unit_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    scar_pipeline: wgpu::RenderPipeline,
    terrain_pipeline: wgpu::RenderPipeline,
    box_pipeline: wgpu::RenderPipeline,
    dome_pipeline: wgpu::RenderPipeline,
    water_pipeline: wgpu::RenderPipeline,
    smoke_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals_buffer: wgpu::Buffer,
    mesh_vertices: wgpu::Buffer,
    mesh_indices: wgpu::Buffer,
    shape_indices: Vec<std::ops::Range<u32>>,
    /// Each shape's far model, drawn in its place once it is small on screen.
    far_indices: Vec<std::ops::Range<u32>>,
    terrain_index_buffer: wgpu::Buffer,
    /// Where each tile's triangles begin and end in the terrain's indices.
    tile_indices: Vec<std::ops::Range<u32>>,
    /// The lowest and highest ground in blocks of the map, for which tiles to draw.
    height_ranges: HeightRanges,
    /// The tiles to draw this frame, for the terrain and for the water.
    tile_runs: TileRuns,
    /// What the shaders read, kept to rebuild their bindings when the shore field changes.
    bind_group_layout: wgpu::BindGroupLayout,
    height_view: wgpu::TextureView,
    fog_view: wgpu::TextureView,
    fog_sampler: wgpu::Sampler,
    shore_view: wgpu::TextureView,
    wave_view: wgpu::TextureView,
    wave_sampler: wgpu::Sampler,
    land_view: wgpu::TextureView,
    detail_view: wgpu::TextureView,
    instance_buffer: InstanceBuffer,
    /// Every shape's instances, one after another, with where each shape's begin and end.
    flat_instances: Vec<UnitInstanceRaw>,
    shape_instances: Vec<std::ops::Range<u32>>,
    /// Where each shape's instances small enough for its far model begin and end.
    far_instances: Vec<std::ops::Range<u32>>,
    samples_per_side: i32,
    /// Samples a side in the terrain grid: the whole map, unless it is larger than the grid.
    grid_size: u32,
    cell_units: f64,
    height_scale: f32,
    /// Ground below this height is drawn as water.
    water_level: f32,
    /// The lowest ground on the map, so that water nowhere above it is not drawn at all.
    lowest_ground: f32,
    /// The ground's heights, for telling where shadows fall when leaving out what is out of sight.
    ground: HeightField,
    icon_scale: f32,
    fog_texture: wgpu::Texture,
    fog_side: u32,
    fog_scale: f32,
    egui: egui_wgpu::Renderer,
    draw_box: bool,
    last_acquire: Duration,
    quality: Quality,
    /// Timing of the scene's parts on the graphics card, once asked for and if it can.
    gpu_timer: Option<GpuTimer>,
}

/// What the scene draws into before it lands in the window: depth, and with more than one
/// sample per pixel, the color that is then averaged down into the window.
struct SceneTargets {
    depth: wgpu::TextureView,
    color: Option<wgpu::TextureView>,
}

impl SceneTargets {
    fn new(device: &wgpu::Device, width: u32, height: u32, color_format: wgpu::TextureFormat, depth_format: wgpu::TextureFormat, samples: u32) -> Self {
        let texture = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        Self { depth: texture("depth", depth_format), color: (samples > 1).then(|| texture("multisampled color", color_format)) }
    }
}

impl Renderer {
    /// Draws ground below this height as water.
    pub fn set_water_level(&mut self, level: f32) {
        self.water_level = level;
        // How far each point is from the water's edge, for the foam and wet sand. Without
        // water, one texel deep inland.
        let started = Instant::now();
        let shore = ShoreField::bake(&self.ground, f64::from(level));
        log::info!("shore field baked in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
        let (side, bytes) = shore.map_or((1, vec![0]), |field| (field.side() as u32, field.bytes()));
        self.shore_view = r8_texture(&self.device, &self.queue, "shore", side, &bytes);
        // How the sun and sky fall on the ground, and where it is damp.
        let started = Instant::now();
        let land = LandField::bake(&self.ground, super::land::SUN_DIRECTION, f64::from(level));
        log::info!("land map baked in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
        self.land_view = rgba_texture(&self.device, &self.queue, "land", &[(land.side() as u32, land.texels())]);
        self.bind_group = make_bind_group(&self.device, &self.bind_group_layout, self);
    }

    /// Draws land and sea at this quality from the next frame on.
    pub fn set_quality(&mut self, quality: Quality) {
        self.quality = quality;
    }

    pub const fn quality(&self) -> Quality {
        self.quality
    }

    /// Starts timing the terrain, units, and water on the graphics card, for test runs. Whether
    /// the adapter can.
    pub fn start_gpu_timing(&mut self) -> bool {
        self.gpu_timer = GpuTimer::new(&self.device, &self.queue);
        self.gpu_timer.is_some()
    }

    /// The average milliseconds the terrain, units, and water took on the graphics card, in
    /// that order, and over how many frames, if timing was started and has read any.
    pub fn gpu_timing(&self) -> Option<([f64; 3], u32)> {
        let timer = self.gpu_timer.as_ref()?;
        timer.averages_ms().map(|averages| (averages, timer.frames()))
    }

    /// `fog_side` is the size of the fog of war grid, if the session has fog.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(window: Arc<Window>, heights: &HeightField, fog_side: Option<usize>) -> Self {
        pollster::block_on(Self::new_async(window, heights, fog_side))
    }

    /// As `new`, for where waiting on the graphics device must not block, as in the browser.
    pub async fn new_async(window: Arc<Window>, heights: &HeightField, fog_side: Option<usize>) -> Self {
        let size = window.inner_size();
        let icon_scale = window.scale_factor() as f32;
        // In the browser, WebGL2, which every current browser has. This version of wgpu asks
        // WebGPU for a limit that browsers have since dropped, so they refuse the device.
        let backends = if cfg!(target_arch = "wasm32") { wgpu::Backends::GL } else { wgpu::Backends::PRIMARY };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let surface = instance.create_surface(window).expect("window supports a surface");
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await
        .expect("a graphics adapter is available");
        log::info!("adapter: {:?}", adapter.get_info());
        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("device"),
                required_features: adapter.features() & (wgpu::Features::DEPTH32FLOAT_STENCIL8 | GpuTimer::FEATURES),
                // A browser's GPU may offer less than the native defaults, as WebGL2 does: ask for
                // what it has, which is more than the game needs.
                required_limits: if cfg!(target_arch = "wasm32") { adapter.limits() } else { wgpu::Limits::default() },
            },
            None,
        ).await
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
        let depth_format = if device.features().contains(wgpu::Features::DEPTH32FLOAT_STENCIL8) {
            wgpu::TextureFormat::Depth32FloatStencil8
        } else {
            wgpu::TextureFormat::Depth24PlusStencil8
        };
        let supports = |format| adapter.get_texture_format_features(format).flags.sample_count_supported(MSAA_SAMPLES);
        let msaa_samples = if supports(format) && supports(depth_format) { MSAA_SAMPLES } else { 1 };
        let multisample = wgpu::MultisampleState { count: msaa_samples, ..Default::default() };

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

        // The fog of war, sampled with smoothing. Without fog, one white texel leaves terrain unchanged.
        let fog_extent = fog_side.map_or(1, |side| side as u32);
        let fog_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fog of war"),
            size: wgpu::Extent3d { width: fog_extent, height: fog_extent, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let fog_view = fog_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let fog_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fog of war"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        queue.write_texture(
            fog_texture.as_image_copy(),
            &vec![255u8; (fog_extent * fog_extent) as usize],
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(fog_extent), rows_per_image: Some(fog_extent) },
            wgpu::Extent3d { width: fog_extent, height: fog_extent, depth_or_array_layers: 1 },
        );
        let fog_scale = fog_side.map_or(0.0, |side| (heights.cell_units() / (side as f64 * FOG_CELL_UNITS)) as f32);
        let egui = egui_wgpu::Renderer::new(&device, format, None, 1);

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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The shore field, sampled with the fog's sampler.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // The small waves' slopes, tiled.
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The land map, sampled with the fog's sampler, and read by units too.
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // The ground's close-up patterns, tiled with the waves' sampler.
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let shore_view = r8_texture(&device, &queue, "shore", 1, &[0]);
        let levels = wave_levels();
        let wave_view = rgba_texture(&device, &queue, "waves", &levels.iter().map(|level| (level.side, level.texels.as_slice())).collect::<Vec<_>>());
        let levels = detail_levels();
        let detail_view = rgba_texture(&device, &queue, "ground detail", &levels.iter().map(|level| (level.side, level.texels.as_slice())).collect::<Vec<_>>());
        // Fully lit and open, until the map's own is baked.
        let land_view = rgba_texture(&device, &queue, "land", &[(1, &[255, 255, 0, 255])]);
        let wave_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("waves"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &bind_group_layout,
            entries: &bind_group_entries(&Bindings {
                globals: &globals_buffer,
                heights: &height_view,
                fog: &fog_view,
                fog_sampler: &fog_sampler,
                shore: &shore_view,
                waves: &wave_view,
                wave_sampler: &wave_sampler,
                land: &land_view,
                detail: &detail_view,
            }),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders.wgsl"));

        let depth = |write: bool, compare| {
            Some(wgpu::DepthStencilState {
                format: depth_format,
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

        let mesh_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 9 => Uint32, 10 => Float32x3];
        let instance_attributes = wgpu::vertex_attr_array![
            2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Unorm8x4, 8 => Unorm8x4
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
                        array_stride: std::mem::size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &mesh_attributes,
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
            multisample,
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
            multisample,
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
            multisample,
            multiview: None,
        });

        // Shadows darken the ground already drawn, each pixel only once however many shapes
        // cover it: the first to reach a pixel marks it in the stencil, and the rest skip it.
        let darken = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            write_mask: wgpu::ColorWrites::COLOR,
        })];
        let once = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::IncrementClamp,
        };
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadows"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &mesh_attributes,
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
                entry_point: "fs_shadow",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &darken,
            }),
            // Flattened onto the ground, a shape's faces overlap both ways up.
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: wgpu::StencilState { front: once, back: once, read_mask: 0xff, write_mask: 0xff },
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample,
            multiview: None,
        });

        // Craters and scorch marks, blended onto the ground before the shadows fall on them.
        let blended_scars = [Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::COLOR })];
        let scar_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("battle scars"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_scar",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &mesh_attributes,
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
                entry_point: "fs_scar",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &blended_scars,
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: depth(false, wgpu::CompareFunction::Greater),
            multisample,
            multiview: None,
        });

        // See-through, so drawn last, tested against the scene but leaving its depth alone.
        let blended = [Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })];
        let dome_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shield domes"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_dome",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &mesh_attributes,
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
                entry_point: "fs_dome",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &blended,
            }),
            // Only the near side, so that the far side does not haze over what is inside.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: depth(false, wgpu::CompareFunction::Greater),
            multisample,
            multiview: None,
        });

        // The water's surface: the terrain grid again, flat at the water's height, see-through,
        // and hidden wherever the ground stands above it.
        let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_water",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_water",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                // The water's color comes already multiplied by how much it covers, so that its
                // sun glints add light over the seabed instead of hiding it.
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::Greater),
            multisample,
            multiview: None,
        });

        // Smoke: lit like the units, but see-through, over everything solid.
        let smoke_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smoke"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_smoke",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &mesh_attributes,
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
                entry_point: "fs_smoke",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &blended,
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: depth(false, wgpu::CompareFunction::Greater),
            multisample,
            multiview: None,
        });

        let meshes = MeshSet::build();
        let mesh_vertices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&meshes.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mesh_indices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&meshes.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        // As many samples a side as the map has, so that the ground, and the shore where the
        // water meets it, is drawn at full detail at every zoom. A coarser grid zoomed out drew
        // a curving coast as straight lines, which changed shape as the camera came down.
        let grid_size = (heights.samples() as u32).clamp(2, MAX_TERRAIN_GRID);
        let (grid_indices, tile_indices) = tiled_indices(grid_size);
        let terrain_index_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("terrain indices"),
            contents: bytemuck::cast_slice(&grid_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let height_ranges = HeightRanges::new(heights.samples(), heights.heights());

        let targets = SceneTargets::new(&device, config.width, config.height, format, depth_format, msaa_samples);
        let instance_buffer = InstanceBuffer::new(&device, 16_384);
        Self {
            surface,
            device,
            queue,
            config,
            depth_format,
            samples: msaa_samples,
            targets,
            unit_pipeline,
            shadow_pipeline,
            scar_pipeline,
            terrain_pipeline,
            box_pipeline,
            dome_pipeline,
            water_pipeline,
            smoke_pipeline,
            bind_group,
            globals_buffer,
            mesh_vertices,
            mesh_indices,
            shape_indices: meshes.ranges,
            far_indices: meshes.far_ranges,
            terrain_index_buffer,
            tile_runs: TileRuns { terrain: Vec::new(), water: Vec::new() },
            tile_indices,
            height_ranges,
            bind_group_layout,
            height_view,
            fog_view,
            fog_sampler,
            shore_view,
            wave_view,
            wave_sampler,
            land_view,
            detail_view,
            instance_buffer,
            flat_instances: Vec::new(),
            shape_instances: Vec::with_capacity(Shape::COUNT),
            far_instances: Vec::with_capacity(Shape::COUNT),
            samples_per_side: heights.samples() as i32,
            grid_size,
            cell_units: heights.cell_units(),
            height_scale: heights.units_per_step() as f32,
            water_level: -1.0e6,
            lowest_ground: heights.heights().iter().copied().min().map_or(0.0, |lowest| f32::from(lowest) * heights.units_per_step() as f32),
            ground: HeightField::new(heights.samples(), heights.heights().to_vec(), heights.cell_units(), heights.units_per_step()),
            icon_scale,
            fog_texture,
            fog_side: fog_extent,
            fog_scale,
            egui,
            draw_box: false,
            last_acquire: Duration::ZERO,
            quality: Quality::default_for_platform(),
            gpu_timer: None,
        }
    }

    /// Uploads the latest fog of war, one byte per cell.
    pub fn update_fog(&self, bytes: &[u8]) {
        let side = self.fog_side;
        if bytes.len() == (side * side) as usize {
            self.queue.write_texture(
                self.fog_texture.as_image_copy(),
                bytes,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(side), rows_per_image: Some(side) },
                wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            );
        }
    }

    /// Uploads textures for a frame's interface. Call once per frame, before drawing it.
    pub fn begin_ui(&mut self, frame: &UiFrame) {
        for (id, delta) in &frame.textures.set {
            self.egui.update_texture(&self.device, &self.queue, *id, delta);
        }
    }

    /// Releases what a frame's interface no longer needs. Call once per frame, after drawing it.
    pub fn end_ui(&mut self, frame: &UiFrame) {
        for id in &frame.textures.free {
            self.egui.free_texture(id);
        }
    }

    /// Records the interface over whatever is already in the target.
    fn encode_ui(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, frame: &UiFrame) -> Vec<wgpu::CommandBuffer> {
        let extra = self.egui.update_buffers(&self.device, &self.queue, encoder, &frame.primitives, &frame.screen);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("interface"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        self.egui.render(&mut pass, &frame.primitives, &frame.screen);
        drop(pass);
        extra
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
        self.targets = SceneTargets::new(&self.device, width, height, self.config.format, self.depth_format, self.samples);
    }

    fn prepare(&mut self, scene: &FrameScene<'_>) {
        let camera = scene.camera;
        let eye = camera.eye();
        let viewport = camera.viewport();

        // A grid covering the whole map draws every sample. Only on a map too big for it does
        // its spacing double with altitude, its first sample snapping to that spacing so that
        // hills do not shimmer as the camera moves.
        let (first, step) = if self.grid_size as i32 >= self.samples_per_side {
            ([0, 0], 1)
        } else {
            let cells = f64::from(self.grid_size - 1);
            let wanted_reach = camera.altitude() * 16.0;
            let wanted_step = (wanted_reach / (cells * self.cell_units)).ceil().max(1.0) as i32;
            let step = (wanted_step as u32).next_power_of_two().min((self.samples_per_side - 1).max(1) as u32) as i32;
            let focus = camera.focus();
            let half_span = (self.grid_size as i32 / 2) * step;
            let center = |value: f64| (value / self.cell_units) as i32;
            let snapped = |value: i32| (value - half_span).div_euclid(step) * step;
            ([snapped(center(focus[0])), snapped(center(focus[1]))], step)
        };

        let selection_box = scene.selection_box.map_or([0.0; 4], |(min, max)| {
            let x = |value: f64| (value / viewport[0] * 2.0 - 1.0) as f32;
            let y = |value: f64| (1.0 - value / viewport[1] * 2.0) as f32;
            [x(min[0]), y(max[1]), x(max[0]), y(min[1])]
        });
        self.draw_box = scene.selection_box.is_some();

        let globals = Globals {
            view_proj: camera.view_projection().to_cols_array_2d(),
            viewport_lod: [viewport[0] as f32, viewport[1] as f32, LOD_DOT_RATIO, LOD_CUBE_RATIO],
            terrain_origin: [
                (f64::from(first[0]) * self.cell_units - eye[0]) as f32,
                (f64::from(first[1]) * self.cell_units - eye[1]) as f32,
                eye[2] as f32,
                self.cell_units as f32,
            ],
            terrain_grid: [first[0], first[1], step, self.samples_per_side],
            selection_box,
            terrain_scale: [self.height_scale, self.icon_scale, self.fog_scale, self.water_level],
            water: [scene.time, self.grid_size as f32, strategic_share(camera.zoom_from_closest()), self.quality.shader_value()],
        };
        self.queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        let placement = GridPlacement { first, step, size: self.grid_size, cell_units: self.cell_units, height_scale: f64::from(self.height_scale) };
        let water_level = if self.water_level > self.lowest_ground { f64::from(self.water_level) } else { f64::NEG_INFINITY };
        self.tile_runs = visible_tiles(&placement, &self.height_ranges, &self.tile_indices, camera.view_projection(), eye, water_level);
        // Only what can show in the picture is sent: the rest of the map's units, scenery, and
        // effects would be worked through by the graphics card and then thrown away.
        let sight = Sight::new(camera.view_projection(), viewport[1] as f32, self.icon_scale);
        let water = f64::from(self.water_level);
        let heights = &self.ground;
        let ground = |x: f32, y: f32| (heights.height_at(f64::from(x) + eye[0], f64::from(y) + eye[1]).max(water) - eye[2]) as f32;
        self.flat_instances.clear();
        self.shape_instances.clear();
        self.far_instances.clear();
        for shape in Shape::ALL {
            let presence = match shape {
                Shape::Scar => Presence::OnGround,
                shape if shape.casts_shadow() => Presence::Shadowed,
                _ => Presence::Solid,
            };
            // Those small on screen are drawn from the shape's far model, if it has one: a few
            // boxes in place of hundreds of triangles that would each cover less than a pixel.
            // (Scars, smoke, and domes are drawn their own ways, and never as dots.)
            let has_far = self.far_indices[shape as usize] != self.shape_indices[shape as usize] && !matches!(shape, Shape::Dome | Shape::Scar | Shape::Smoke);
            let seen = || scene.instances.shape(shape).iter().filter(|part| sight.sees(part, presence, ground));
            let start = self.flat_instances.len() as u32;
            self.flat_instances.extend(seen().filter(|part| !(has_far && is_small(part))));
            let far_start = self.flat_instances.len() as u32;
            self.shape_instances.push(start..far_start);
            if has_far {
                self.flat_instances.extend(seen().filter(|part| is_small(part)));
            }
            self.far_instances.push(far_start..self.flat_instances.len() as u32);
        }
        self.instance_buffer.write(&self.device, &self.queue, &self.flat_instances);
    }

    fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, scene_targets: &SceneTargets) {
        // With several samples per pixel, they are averaged into the target as the pass ends.
        let (view, resolve_target, store) = match &scene_targets.color {
            Some(color) => (color, Some(target), wgpu::StoreOp::Discard),
            None => (target, None, wgpu::StoreOp::Store),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(BACKGROUND), store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &scene_targets.depth,
                // Reversed Z: the far plane is 0.
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        let timer = self.gpu_timer.as_ref();
        let stamp = |pass: &mut wgpu::RenderPass<'_>, index| {
            if let Some(timer) = timer {
                timer.stamp(pass, index);
            }
        };

        stamp(&mut pass, 0);
        pass.set_pipeline(&self.terrain_pipeline);
        pass.set_index_buffer(self.terrain_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for run in &self.tile_runs.terrain {
            pass.draw_indexed(run.clone(), 0, 0..1);
        }
        stamp(&mut pass, 1);

        pass.set_vertex_buffer(0, self.mesh_vertices.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.buffer.slice(..));
        pass.set_index_buffer(self.mesh_indices.slice(..), wgpu::IndexFormat::Uint32);
        // Battle scars go on the bare ground, under the shadows.
        let scars = &self.shape_instances[Shape::Scar as usize];
        if !scars.is_empty() {
            pass.set_pipeline(&self.scar_pipeline);
            pass.draw_indexed(self.shape_indices[Shape::Scar as usize].clone(), 0, scars.clone());
        }
        // Shadows go on the bare ground, before the units that cast them stand over them.
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_stencil_reference(0);
        for ((indices, instances), shape) in self.shape_indices.iter().zip(&self.shape_instances).zip(Shape::ALL) {
            if !instances.is_empty() && shape.casts_shadow() {
                pass.draw_indexed(indices.clone(), 0, instances.clone());
            }
        }
        for ((indices, instances), shape) in self.far_indices.iter().zip(&self.far_instances).zip(Shape::ALL) {
            if !instances.is_empty() && shape.casts_shadow() {
                pass.draw_indexed(indices.clone(), 0, instances.clone());
            }
        }
        pass.set_pipeline(&self.unit_pipeline);
        for ((indices, instances), shape) in self.shape_indices.iter().zip(&self.shape_instances).zip(Shape::ALL) {
            if !instances.is_empty() && !matches!(shape, Shape::Dome | Shape::Scar | Shape::Smoke) {
                pass.draw_indexed(indices.clone(), 0, instances.clone());
            }
        }
        for (indices, instances) in self.far_indices.iter().zip(&self.far_instances) {
            if !instances.is_empty() {
                pass.draw_indexed(indices.clone(), 0, instances.clone());
            }
        }
        stamp(&mut pass, 2);
        // Water over the units, so that what is under it shows through, tinted by its depth.
        // Only over the tiles in the picture with ground under the water.
        if !self.tile_runs.water.is_empty() {
            pass.set_pipeline(&self.water_pipeline);
            pass.set_index_buffer(self.terrain_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            for run in &self.tile_runs.water {
                pass.draw_indexed(run.clone(), 0, 0..1);
            }
            pass.set_index_buffer(self.mesh_indices.slice(..), wgpu::IndexFormat::Uint32);
        }
        stamp(&mut pass, 3);
        let smoke = &self.shape_instances[Shape::Smoke as usize];
        if !smoke.is_empty() {
            pass.set_pipeline(&self.smoke_pipeline);
            pass.draw_indexed(self.shape_indices[Shape::Smoke as usize].clone(), 0, smoke.clone());
        }
        let domes = &self.shape_instances[Shape::Dome as usize];
        if !domes.is_empty() {
            pass.set_pipeline(&self.dome_pipeline);
            pass.draw_indexed(self.shape_indices[Shape::Dome as usize].clone(), 0, domes.clone());
        }

        if self.draw_box {
            pass.set_pipeline(&self.box_pipeline);
            pass.draw(0..24, 0..1);
        }
    }

    pub fn render(&mut self, scene: &FrameScene<'_>, ui: Option<&UiFrame>) {
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
        self.encode(&mut encoder, &view, &self.targets);
        if let Some(timer) = &mut self.gpu_timer {
            timer.resolve(&mut encoder);
        }
        let extra = ui.map_or_else(Vec::new, |ui| self.encode_ui(&mut encoder, &view, ui));
        self.queue.submit(extra.into_iter().chain([encoder.finish()]));
        if let Some(timer) = &mut self.gpu_timer {
            timer.after_submit(&self.device);
        }
        frame.present();
    }

    /// Draws a scene offscreen and saves it as a 24-bit BMP, for test runs.
    pub fn capture(&mut self, scene: &FrameScene<'_>, ui: Option<&UiFrame>, path: &Path) -> io::Result<()> {
        let (width, height, pixels) = self.capture_rgba(scene, ui);
        fs::write(path, encode_bmp(&pixels, width, height, width * 4, true))
    }

    /// Draws a scene offscreen and returns its width, height, and pixels, four bytes each in
    /// red, green, blue, alpha order, row by row from the top.
    pub fn capture_rgba(&mut self, scene: &FrameScene<'_>, ui: Option<&UiFrame>) -> (u32, u32, Vec<u8>) {
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
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("capture") });
        self.encode(&mut encoder, &view, &self.targets);
        let extra = ui.map_or_else(Vec::new, |ui| self.encode_ui(&mut encoder, &view, ui));
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(height) },
            },
            extent,
        );
        self.queue.submit(extra.into_iter().chain([encoder.finish()]));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("capture buffer maps"));
        self.device.poll(wgpu::Maintain::Wait);
        let is_rgba = matches!(self.config.format, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        {
            let mapped = slice.get_mapped_range();
            for y in 0..height {
                let row = &mapped[(y * bytes_per_row) as usize..(y * bytes_per_row + width * 4) as usize];
                for pixel in row.as_chunks::<4>().0 {
                    if is_rgba {
                        pixels.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                    } else {
                        pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                    }
                }
            }
        }
        readback.unmap();
        (width, height, pixels)
    }
}

/// Encodes 4-byte pixels as a top-down 24-bit BMP. BMP stores blue first.
/// A one-byte-a-texel texture, `side` texels a side, filled with `bytes`, and its view.
fn r8_texture(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, side: u32, bytes: &[u8]) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // Rows of a texture write need not be aligned, only those of a buffer copy.
    queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(side), rows_per_image: Some(side) },
        wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A four-byte-a-texel texture from its mip levels, largest first, each `(side, texels)`, and its
/// view.
fn rgba_texture(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, levels: &[(u32, &[u8])]) -> wgpu::TextureView {
    let side = levels[0].0;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip_level, &(side, texels)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::ImageCopyTexture { texture: &texture, mip_level: mip_level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            texels,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(side * 4), rows_per_image: Some(side) },
            wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// What the shaders read, in binding order.
struct Bindings<'a> {
    globals: &'a wgpu::Buffer,
    heights: &'a wgpu::TextureView,
    fog: &'a wgpu::TextureView,
    fog_sampler: &'a wgpu::Sampler,
    shore: &'a wgpu::TextureView,
    waves: &'a wgpu::TextureView,
    wave_sampler: &'a wgpu::Sampler,
    land: &'a wgpu::TextureView,
    detail: &'a wgpu::TextureView,
}

fn bind_group_entries<'a>(bindings: &Bindings<'a>) -> [wgpu::BindGroupEntry<'a>; 9] {
    [
        wgpu::BindGroupEntry { binding: 0, resource: bindings.globals.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(bindings.heights) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(bindings.fog) },
        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(bindings.fog_sampler) },
        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(bindings.shore) },
        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(bindings.waves) },
        wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(bindings.wave_sampler) },
        wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(bindings.land) },
        wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(bindings.detail) },
    ]
}

fn make_bind_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, renderer: &Renderer) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout,
        entries: &bind_group_entries(&Bindings {
            globals: &renderer.globals_buffer,
            heights: &renderer.height_view,
            fog: &renderer.fog_view,
            fog_sampler: &renderer.fog_sampler,
            shore: &renderer.shore_view,
            waves: &renderer.wave_view,
            wave_sampler: &renderer.wave_sampler,
            land: &renderer.land_view,
            detail: &renderer.detail_view,
        }),
    })
}

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

    /// A part is drawn from its far model from where the shader begins to turn it into its dot,
    /// and one that never becomes a dot, such as a foundation, never is.
    #[test]
    fn a_part_is_small_from_where_it_begins_to_turn_into_its_dot() {
        let part = |distance: f32, icon: f32| UnitInstanceRaw {
            model_matrix: [[0.0, -18.0, 0.0, 0.0], [18.0, 0.0, 0.0, 0.0], [0.0, 0.0, 18.0, 0.0], [0.0, 0.0, -distance, 1.0]],
            color: [1.0, 1.0, 1.0, icon],
            hull: [0; 4],
            glow: [0; 4],
        };
        let begins = 18.0 / LOD_CUBE_RATIO;
        assert!(!is_small(&part(begins * 0.99, 2.5)), "still wholly its model");
        assert!(is_small(&part(begins * 1.01, 2.5)));
        assert!(is_small(&part(18.0 / LOD_DOT_RATIO * 2.0, 2.5)), "and all the way to a dot");
        assert!(!is_small(&part(begins * 100.0, 0.0)), "what has no dot is never small");
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
