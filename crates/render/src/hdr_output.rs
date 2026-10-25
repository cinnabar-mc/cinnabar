//! HDR display output. On request the primary window's swapchain switches to scRGB or HDR10,
//! the world is expanded or tonemapped into the display's headroom, screen UI composites at
//! paper white, and the final frame is encoded for the chosen signal.

use bevy::{
    app::{App, SubApp},
    asset::{Handle, load_internal_asset, uuid_handle},
    ecs::{
        entity::Entity,
        error::BevyError,
        query::With,
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Local, Query, Res},
        world::{FromWorld, World},
    },
    log::info,
    render::{
        Render, RenderSystems,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingType, BufferBindingType, BufferInitDescriptor, BufferSize, BufferUsages,
            CachedRenderPipelineId, Canonical, ColorTargetState, ColorWrites, FragmentState,
            PipelineCache, RenderPipeline, RenderPipelineDescriptor, ShaderStages, Specializer,
            SpecializerKey, TextureFormat, TextureSampleType, TextureView, TextureViewDimension,
            Variants, VertexState,
        },
        renderer::{RenderAdapter, RenderDevice},
        view::window::{ExtractedWindow, SurfaceData, create_surfaces, prepare_windows},
    },
    shader::Shader,
    utils::default,
    window::PrimaryWindow,
};
use render_api::hdr::{HDR_EXPANSION_KNEE, HdrEncoding, HdrOutput};
use wgpu::{DisplayHdrInfo, SurfaceCapabilities, SurfaceColorSpace};

#[cfg(test)]
#[path = "hdr_output/tests.rs"]
mod tests;

const SHADER: Handle<Shader> = uuid_handle!("8f0a6c0e-31c7-4b54-9d8e-2b3b1f8f5a17");
/// Frames between display re-probes while HDR is requested, so a toggle of the system HDR mode
/// or a move to another monitor takes effect without polling every frame.
const DISPLAY_PROBE_FRAMES: u32 = 120;

/// The player's HDR request, inserted by the app during extraction. Whether a view actually
/// presents HDR follows from its output format; see [`output_encoding`].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct HdrOutputRequest(pub HdrOutput);

/// The swapchain format and colour space that carry `encoding`.
pub(crate) const fn swapchain(encoding: HdrEncoding) -> (TextureFormat, SurfaceColorSpace) {
    match encoding {
        HdrEncoding::Scrgb => (
            TextureFormat::Rgba16Float,
            SurfaceColorSpace::ExtendedSrgbLinear,
        ),
        HdrEncoding::Pq => (TextureFormat::Rgb10a2Unorm, SurfaceColorSpace::Bt2100Pq),
    }
}

/// The HDR encoding a camera output of `format` expects, or `None` for SDR outputs.
///
/// Only [`configure_hdr_surface`] selects these formats for a window, so the output format
/// alone identifies an HDR frame, including Bevy's screenshot copies of it.
pub fn output_encoding(format: TextureFormat) -> Option<HdrEncoding> {
    [HdrEncoding::Scrgb, HdrEncoding::Pq]
        .into_iter()
        .find(|&encoding| swapchain(encoding).0 == format)
}

/// Prefers scRGB, which keeps half-float precision and BT.709 primaries, then HDR10. A display
/// that reports SDR mode gets neither: its compositor would clip everything above paper white.
pub(crate) fn choose_encoding(
    capabilities: &SurfaceCapabilities,
    display: &DisplayHdrInfo,
) -> Option<HdrEncoding> {
    if display.coarse.and_then(|range| range.high_dynamic_range) == Some(false) {
        return None;
    }
    [HdrEncoding::Scrgb, HdrEncoding::Pq]
        .into_iter()
        .find(|&encoding| {
            let (format, space) = swapchain(encoding);
            space
                .to_color_spaces()
                .is_some_and(|space| capabilities.color_spaces(format).contains(space))
        })
}

/// Installs the surface switch and the expansion and encoding shader.
pub(crate) fn install(app: &mut App) {
    load_internal_asset!(
        app,
        SHADER,
        "hdr_output.wgsl",
        crate::shader_safety::from_wgsl
    );
    let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
        return;
    };
    install_render(render_app);
}

fn install_render(render_app: &mut SubApp) {
    render_app
        .init_resource::<HdrOutputRequest>()
        .init_resource::<HdrStagePipeline>()
        .add_systems(
            Render,
            configure_hdr_surface
                .in_set(RenderSystems::PrepareViews)
                .after(create_surfaces)
                .before(prepare_windows),
        );
}

/// The primary window's last SDR configuration and the cached display probe.
#[derive(Default)]
struct SurfaceMemory {
    sdr: Option<(Entity, TextureFormat, SurfaceColorSpace)>,
    probe: Option<Option<HdrEncoding>>,
    frames: u32,
}

/// Switches the primary window between Bevy's SDR swapchain and the requested HDR signal.
fn configure_hdr_surface(
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy::ecs::system::NonSendMarker,
    request: Res<HdrOutputRequest>,
    adapter: Res<RenderAdapter>,
    device: Res<RenderDevice>,
    mut windows: Query<(Entity, &mut ExtractedWindow, &mut SurfaceData), With<PrimaryWindow>>,
    mut memory: Local<SurfaceMemory>,
) {
    let output = request.0;
    let Ok((entity, mut window, mut surface)) = windows.single_mut() else {
        return;
    };
    let current = (
        surface.configuration().format,
        surface.configuration().color_space,
    );
    if !current.1.is_hdr() {
        memory.sdr = Some((entity, current.0, current.1));
    }
    memory.frames = memory.frames.wrapping_add(1);
    let desired = if output.enabled {
        let stale = memory.probe.is_none()
            || window.size_changed
            || memory.frames.is_multiple_of(DISPLAY_PROBE_FRAMES);
        if stale {
            let capabilities = surface.surface().get_capabilities(&adapter);
            let display = surface.surface().display_hdr_info(&adapter);
            let encoding = choose_encoding(&capabilities, &display);
            if memory.probe != Some(encoding) {
                match encoding {
                    Some(encoding) => info!(
                        "HDR output: {encoding:?}, peak {} nits, paper white {} nits",
                        output.peak_nits, output.paper_white_nits
                    ),
                    None => info!("HDR output requested but this display path is SDR"),
                }
            }
            memory.probe = Some(encoding);
        }
        memory.probe.flatten()
    } else {
        memory.probe = None;
        None
    };
    let target = match desired {
        Some(encoding) => Some(swapchain(encoding)),
        None if current.1.is_hdr() => memory
            .sdr
            .filter(|(owner, ..)| *owner == entity)
            .map(|(_, format, space)| (format, space)),
        None => None,
    };
    if let Some((format, space)) = target.filter(|&target| target != current) {
        // Bevy drops the acquired frame the same way before it reconfigures for a resize.
        window.swap_chain_texture_view = None;
        window.swap_chain_texture = None;
        surface.set_format(&device, format, space);
    }
}

/// Uniform values for the expansion and encoding passes; mirrors `HdrOutput` in
/// `hdr_output.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct HdrStageConstants {
    paper_white_nits: f32,
    sdr_white: f32,
    knee: f32,
    _padding: f32,
}

impl HdrStageConstants {
    /// Expands SDR-tonemapped scenes when `expand`; HDR-tonemapped scenes keep their range.
    pub(crate) fn new(output: HdrOutput, expand: bool) -> Self {
        Self {
            paper_white_nits: output.paper_white_nits.max(1.0),
            sdr_white: if expand { output.sdr_white() } else { 1.0 },
            knee: HDR_EXPANSION_KNEE,
            _padding: 0.0,
        }
    }

    /// A cache key that changes whenever the uniform contents do.
    pub(crate) fn key(self) -> [u32; 3] {
        [
            self.paper_white_nits.to_bits(),
            self.sdr_white.to_bits(),
            self.knee.to_bits(),
        ]
    }
}

/// Which full-screen HDR pass a pipeline runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HdrStage {
    /// Copies a scene into the float UI target, expanding SDR highlights.
    Expand,
    /// Writes the composed frame as the output's HDR signal.
    Encode(HdrEncoding),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(crate) struct HdrStageKey {
    pub(crate) stage: HdrStage,
    pub(crate) format: TextureFormat,
}

struct HdrStageSpecializer;

impl Specializer<RenderPipeline> for HdrStageSpecializer {
    type Key = HdrStageKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.targets[0].as_mut().unwrap().format = key.format;
        fragment.entry_point = Some(
            match key.stage {
                HdrStage::Expand => "expand_fragment",
                HdrStage::Encode(HdrEncoding::Scrgb) => "encode_scrgb_fragment",
                HdrStage::Encode(HdrEncoding::Pq) => "encode_pq_fragment",
            }
            .into(),
        );
        Ok(key)
    }
}

/// The shared layout and cached variants of the HDR expansion and encoding passes.
#[derive(Resource)]
pub(crate) struct HdrStagePipeline {
    layout: BindGroupLayoutDescriptor,
    variants: Variants<RenderPipeline, HdrStageSpecializer>,
}

impl FromWorld for HdrStagePipeline {
    fn from_world(_world: &mut World) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "HDR output stage",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(size_of::<HdrStageConstants>() as u64),
                    },
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("HDR output stage".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: SHADER,
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: SHADER,
                entry_point: Some("expand_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_HDR_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        };
        Self {
            layout,
            variants: Variants::new(HdrStageSpecializer, descriptor),
        }
    }
}

impl HdrStagePipeline {
    /// Queues one reusable variant for a stage and its colour target.
    pub(crate) fn specialize(
        &mut self,
        cache: &PipelineCache,
        key: HdrStageKey,
    ) -> CachedRenderPipelineId {
        self.variants
            .specialize(cache, key)
            .expect("valid HDR output stage specialization")
    }

    /// Binds a same-sized source image and the stage's constants.
    pub(crate) fn binding(
        &self,
        device: &RenderDevice,
        cache: &PipelineCache,
        source: &TextureView,
        constants: HdrStageConstants,
    ) -> BindGroup {
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("HDR output stage constants"),
            contents: bytemuck::bytes_of(&constants),
            usage: BufferUsages::UNIFORM,
        });
        device.create_bind_group(
            "HDR output stage",
            &cache.get_bind_group_layout(&self.layout),
            &BindGroupEntries::sequential((source, buffer.as_entire_binding())),
        )
    }
}
