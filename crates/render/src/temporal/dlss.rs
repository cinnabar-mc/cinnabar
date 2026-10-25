//! DLSS Super Resolution: reconstructs the jittered render-resolution world at output
//! resolution from colour, depth and motion vectors. Vulkan only; see docs/temporal-upscaling.md.

use std::sync::{Arc, Mutex};

use bevy::{
    app::SubApp,
    ecs::{
        component::Component,
        entity::Entity,
        query::{Or, With, Without},
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, Local, Query, Res},
        world::World,
    },
    log::{info, warn},
    render::{
        Render, RenderSystems,
        render_resource::{
            Extent3d, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
            TextureView,
        },
        renderer::{RenderAdapter, RenderContext, RenderDevice, RenderQueue},
        view::{ViewDepthStencilTexture, ViewTarget},
    },
    utils::default,
};
use dlss_wgpu::{
    DlssFeatureFlags, DlssPerfQualityMode, DlssSdk,
    super_resolution::{
        DlssSuperResolution, DlssSuperResolutionExposure, DlssSuperResolutionRenderParameters,
    },
};
use render_scene::temporal::{TemporalUpscaler, TemporalUpscalerSupport, TemporalUpscaling};

use super::TemporalView;

#[cfg(test)]
#[path = "dlss/tests.rs"]
mod tests;

/// NGX samples the world through its view, decoding sRGB, and stores linear colour; a float
/// target keeps that precision until the copy into the native UI scene re-encodes it.
const OUTPUT_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

pub(super) fn install(render_app: &mut SubApp) {
    render_app.add_systems(
        Render,
        prepare_dlss_views
            .in_set(RenderSystems::PrepareBindGroups)
            .after(super::prepare_temporal_views),
    );
}

/// The process-wide NGX session, created on first use.
#[derive(Resource, Clone)]
struct Sdk(Arc<Mutex<DlssSdk>>);

/// What a DLSS feature was created for; any change recreates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DlssKey {
    output: [u32; 2],
    render: [u32; 2],
    mode: DlssPerfQualityMode,
    world: TextureFormat,
}

/// One view's DLSS feature and output.
#[derive(Component)]
pub(crate) struct DlssView {
    key: DlssKey,
    context: Mutex<DlssSuperResolution>,
    /// Written by NGX as storage, then copied into the native UI scene.
    output: TextureView,
}

/// The DLSS quality mode whose render scale is nearest `render / output`; DLAA at full size.
pub(crate) fn mode_for(render: [u32; 2], output: [u32; 2]) -> DlssPerfQualityMode {
    if render == output {
        return DlssPerfQualityMode::Dlaa;
    }
    let ratio = render[0] as f32 / output[0].max(1) as f32;
    [
        (DlssPerfQualityMode::Quality, 2.0 / 3.0),
        (DlssPerfQualityMode::Balanced, 0.58),
        (DlssPerfQualityMode::Performance, 0.5),
        (DlssPerfQualityMode::UltraPerformance, 1.0 / 3.0),
    ]
    .into_iter()
    .min_by(|a, b| (a.1 - ratio).abs().total_cmp(&(b.1 - ratio).abs()))
    .map_or(DlssPerfQualityMode::Quality, |(mode, _)| mode)
}

/// Whether `render` lies inside a feature's supported render-resolution range.
fn admits(range: &std::ops::RangeInclusive<[u32; 2]>, render: [u32; 2]) -> bool {
    (0..2).all(|axis| range.start()[axis] <= render[axis] && render[axis] <= range.end()[axis])
}

type DlssViews<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static TemporalUpscaling,
        &'static ViewTarget,
        Option<&'static DlssView>,
    ),
>;

type RemovedViews<'w, 's> = Query<
    'w,
    's,
    Entity,
    (
        With<DlssView>,
        Or<(Without<TemporalUpscaling>, Without<ViewTarget>)>,
    ),
>;

/// Creates or replaces each DLSS view's feature when its resolutions change. A failure leaves
/// the view without DLSS, which keeps FSR1 reconstruction.
#[expect(
    clippy::too_many_arguments,
    reason = "render resources and view queries are system parameters"
)]
fn prepare_dlss_views(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    support: Res<TemporalUpscalerSupport>,
    sdk: Option<Res<Sdk>>,
    views: DlssViews,
    removed: RemovedViews,
    mut rejected: Local<Option<DlssKey>>,
) {
    for entity in &removed {
        commands.entity(entity).remove::<DlssView>();
    }
    if !support.dlss {
        return;
    }
    let mut sdk = sdk.map(|sdk| sdk.clone());
    for (entity, request, target, current) in &views {
        if request.upscaler != TemporalUpscaler::Dlss {
            if current.is_some() {
                commands.entity(entity).remove::<DlssView>();
            }
            continue;
        }
        let Some(output) = target.out_texture() else {
            continue;
        };
        let output = crate::render_bounds::extent(output);
        let size = target.main_texture().size();
        let render = [size.width, size.height];
        let key = DlssKey {
            output,
            render,
            mode: mode_for(render, output),
            world: target.main_texture_format(),
        };
        if current.is_some_and(|current| current.key == key) || *rejected == Some(key) {
            continue;
        }
        let sdk = match &sdk {
            Some(sdk) => sdk.clone(),
            None => match DlssSdk::new(super::DLSS_PROJECT_ID, device.wgpu_device().clone()) {
                Ok(created) => {
                    let created = Sdk(created);
                    commands.insert_resource(created.clone());
                    sdk = Some(created.clone());
                    created
                }
                Err(error) => {
                    warn!("DLSS is unavailable: {error}");
                    *rejected = Some(key);
                    continue;
                }
            },
        };
        let context = match DlssSuperResolution::new(
            output,
            key.mode,
            DlssFeatureFlags::LowResolutionMotionVectors | DlssFeatureFlags::InvertedDepth,
            sdk.0,
            device.wgpu_device(),
            &queue,
        ) {
            Ok(context) => context,
            Err(error) => {
                warn!("DLSS feature for {output:?} could not be created: {error}");
                *rejected = Some(key);
                continue;
            }
        };
        if !admits(&context.render_resolution_range(), render) {
            warn!(
                "DLSS {:?} at {output:?} accepts render sizes {:?}; {render:?} keeps FSR 1",
                key.mode,
                context.render_resolution_range()
            );
            *rejected = Some(key);
            continue;
        }
        info!("DLSS {:?} reconstructs {render:?} to {output:?}", key.mode);
        let texture: Texture = device.create_texture(&TextureDescriptor {
            label: Some("DLSS output"),
            size: Extent3d {
                width: output[0],
                height: output[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        commands.entity(entity).insert(DlssView {
            key,
            context: Mutex::new(context),
            output: texture.create_view(&default()),
        });
    }
}

/// Records DLSS for `view` and returns its output-resolution reconstruction, or `None` when
/// the view has no ready DLSS feature for its current sizes.
pub(crate) fn evaluate(
    world: &World,
    context: &mut RenderContext,
    view: Entity,
    target: &ViewTarget,
) -> Option<TextureView> {
    let (Some(dlss), Some(inputs), Some(depth), Some(adapter)) = (
        world.get::<DlssView>(view),
        world.get::<TemporalView>(view),
        world.get::<ViewDepthStencilTexture>(view),
        world.get_resource::<RenderAdapter>(),
    ) else {
        return None;
    };
    let render = inputs.render_size();
    let size = target.main_texture().size();
    if dlss.key.render != render
        || render != [size.width, size.height]
        || dlss.key.world != target.main_texture_format()
    {
        return None;
    }
    let mut feature = dlss.context.lock().expect("DLSS feature lock");
    let commands = feature
        .render(
            DlssSuperResolutionRenderParameters {
                color: target.main_texture_view(),
                depth: crate::scene_sampling::view_depth(depth),
                motion_vectors: inputs.motion_vectors(),
                exposure: DlssSuperResolutionExposure::Automatic,
                bias: None,
                dlss_output: &dlss.output,
                reset: inputs.reset,
                // Bevy's jitter offset moves the projection; DLSS takes the sample offset.
                jitter_offset: (-inputs.jitter).to_array(),
                partial_texture_size: Some(render),
                motion_vector_scale: None,
            },
            context.command_encoder(),
            adapter,
        )
        .inspect_err(|error| warn!("DLSS evaluation failed: {error}"))
        .ok()?;
    context.add_command_buffer(commands);
    Some(dlss.output.clone())
}
