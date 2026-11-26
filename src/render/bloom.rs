//! Bloom: an optional glow around light brighter than the screen can show, such as sun glints,
//! explosions and engine lights (bloom.wgsl). Off unless asked for, since it draws the scene
//! into a bright target and filters it several times each frame, which weak graphics feel.
//!
//! Drawing only: the simulation never sees it.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// The bright target the scene is drawn into, untoned, when bloom is on.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Brightness before toning from which light glows: past white on screen, so that sunlit
/// ground and paint stay as they are and only light sources, glints and fire glow.
const THRESHOLD: f32 = 1.2;
/// How softly the glow starts below and above the threshold.
const KNEE: f32 = 0.4;
/// How much of the glow is added to the scene.
const INTENSITY: f32 = 0.8;
/// How far each step's glow spreads, in its texels.
const RADIUS: f32 = 1.0;
/// How many smaller copies the glow is spread through at most, each half the size of the one
/// before, and the fewest texels a side the smallest may have.
const MAX_LEVELS: u32 = 6;
const SMALLEST_SIDE: u32 = 8;
/// How much brighter glowing parts are drawn with bloom on, so that they glow: without it they
/// are drawn at the screen's white at most.
pub const EMISSIVE_GAIN: f32 = 3.0;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Params {
    threshold: f32,
    knee: f32,
    intensity: f32,
    radius: f32,
}

/// Whether the adapter can draw the scene into the bright target, with `samples` per pixel.
pub fn supported(adapter: &wgpu::Adapter, samples: u32) -> bool {
    let features = adapter.get_texture_format_features(HDR_FORMAT);
    let usages = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let flags = wgpu::TextureFormatFeatureFlags::FILTERABLE | wgpu::TextureFormatFeatureFlags::BLENDABLE;
    features.allowed_usages.contains(usages)
        && features.flags.contains(flags)
        && (samples == 1 || (features.flags.sample_count_supported(samples) && features.flags.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE)))
}

/// How many smaller copies a picture this size spreads its glow through.
pub fn levels(width: u32, height: u32) -> u32 {
    let mut side = width.min(height) / 2;
    let mut levels = 0;
    while levels < MAX_LEVELS && side >= SMALLEST_SIDE {
        levels += 1;
        side /= 2;
    }
    levels.max(1)
}

/// What bloom draws into, which changes with the window's size.
struct Targets {
    /// The scene, untoned, with several samples a pixel where the scene has them, and averaged.
    multisampled: Option<wgpu::TextureView>,
    scene: wgpu::TextureView,
    /// The glow, from half size down.
    levels: Vec<wgpu::TextureView>,
    /// Each step's inputs: the bright light taken out, each copy shrunk, each grown back, and
    /// the last step's.
    prefilter: wgpu::BindGroup,
    downsample: Vec<wgpu::BindGroup>,
    upsample: Vec<wgpu::BindGroup>,
    composite: wgpu::BindGroup,
}

pub struct Bloom {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    prefilter: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    samples: u32,
    targets: Targets,
}

impl Bloom {
    /// For a window of `output` format, `width` by `height`, whose scene has `samples` a pixel.
    pub fn new(device: &wgpu::Device, output: wgpu::TextureFormat, width: u32, height: u32, samples: u32) -> Self {
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                texture_entry(3),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bloom"),
            contents: bytemuck::bytes_of(&Params { threshold: THRESHOLD, knee: KNEE, intensity: INTENSITY, radius: RADIUS }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("bloom.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("bloom"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = |label, entry_point, format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: "vs_fullscreen", compilation_options: wgpu::PipelineCompilationOptions::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            })
        };
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let prefilter = pipeline("bloom bright light", "fs_prefilter", HDR_FORMAT, None);
        let downsample = pipeline("bloom shrink", "fs_downsample", HDR_FORMAT, None);
        let upsample = pipeline("bloom grow", "fs_upsample", HDR_FORMAT, Some(add));
        let composite = pipeline("bloom composite", "fs_composite", output, None);
        let targets = Self::targets(device, &layout, &sampler, &params, width, height, samples);
        Self { layout, sampler, params, prefilter, downsample, upsample, composite, samples, targets }
    }

    fn targets(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, params: &wgpu::Buffer, width: u32, height: u32, samples: u32) -> Targets {
        let texture = |label, width: u32, height: u32, samples| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format: HDR_FORMAT,
                    usage: if samples > 1 { wgpu::TextureUsages::RENDER_ATTACHMENT } else { wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING },
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let multisampled = (samples > 1).then(|| texture("bloom scene, multisampled", width, height, samples));
        let scene = texture("bloom scene", width, height, 1);
        let levels: Vec<_> = (1..=levels(width, height)).map(|level| texture("bloom glow", width >> level, height >> level, 1)).collect();
        let group = |source: &wgpu::TextureView, glow: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(glow) },
                ],
            })
        };
        let prefilter = group(&scene, &scene);
        // Shrinking reads each copy to draw the next; growing reads each to add to the one above.
        let downsample = levels.windows(2).map(|pair| group(&pair[0], &pair[0])).collect();
        let upsample = levels.windows(2).map(|pair| group(&pair[1], &pair[1])).collect();
        let composite = group(&scene, &levels[0]);
        Targets { multisampled, scene, levels, prefilter, downsample, upsample, composite }
    }

    /// Draws into a window of this size from the next frame on.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.targets = Self::targets(device, &self.layout, &self.sampler, &self.params, width, height, self.samples);
    }

    /// What the scene draws into: the view to draw, and the view its samples are averaged into
    /// if it has several a pixel.
    pub fn scene_attachment(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        match &self.targets.multisampled {
            Some(multisampled) => (multisampled, Some(&self.targets.scene)),
            None => (&self.targets.scene, None),
        }
    }

    /// Spreads the scene's bright light, and tones the scene and its glow into `output`.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let targets = &self.targets;
        let step = |encoder: &mut wgpu::CommandEncoder, label, view: &wgpu::TextureView, load, pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        step(encoder, "bloom bright light", &targets.levels[0], clear, &self.prefilter, &targets.prefilter);
        for (index, group) in targets.downsample.iter().enumerate() {
            step(encoder, "bloom shrink", &targets.levels[index + 1], clear, &self.downsample, group);
        }
        for (index, group) in targets.upsample.iter().enumerate().rev() {
            step(encoder, "bloom grow", &targets.levels[index], wgpu::LoadOp::Load, &self.upsample, group);
        }
        step(encoder, "bloom composite", output, clear, &self.composite, &targets.composite);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_compiles() {
        let source = include_str!("bloom.wgsl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)));
    }

    #[test]
    fn the_params_match_the_shader() {
        let module = naga::front::wgsl::parse_str(include_str!("bloom.wgsl")).expect("the shader parses");
        let params = module.types.iter().find(|(_, ty)| ty.name.as_deref() == Some("Params")).expect("the shader has Params").1;
        let naga::TypeInner::Struct { span, .. } = params.inner else { panic!("Params is a struct") };
        assert_eq!(span as usize, std::mem::size_of::<Params>());
    }

    /// Down to copies at least 8 texels a side, and never more than six.
    #[test]
    fn the_glow_spreads_through_fewer_copies_on_a_small_window() {
        assert_eq!(levels(1920, 1080), 6);
        assert_eq!(levels(320, 200), 4);
        assert_eq!(levels(16, 16), 1);
        assert_eq!(levels(1, 1), 1);
    }

    /// The tone curve in the last step is the scene shaders' own, so that a scene with no light
    /// past white looks the same with bloom on as off.
    #[test]
    fn the_composite_tones_as_the_scene_does() {
        let tone = |source: &str| {
            let start = source.find("const TONE_KNEE").expect("a tone knee");
            let end = start + source[start..].find("\n}\n").expect("tone ends");
            source[start..end].to_string()
        };
        assert_eq!(tone(include_str!("bloom.wgsl")), tone(include_str!("shaders.wgsl")));
    }
}
