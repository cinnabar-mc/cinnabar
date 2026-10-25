//! Temporal upscaling inputs: per-view camera history, a reset decision, and render-resolution
//! motion vectors, consumed when the reduced world is reconstructed at output resolution (see
//! `ui_render::target`). With the `dlss` feature, DLSS Super Resolution is the consumer.

use std::sync::Mutex;

use bevy::{
    app::{App, Plugin},
    asset::{Handle, load_internal_asset, uuid_handle},
    camera::Camera3d,
    core_pipeline::{Core3d, Core3dSystems},
    ecs::{
        entity::Entity,
        query::{Or, With, Without},
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, Query, Res},
        world::World,
    },
    math::{Mat4, Vec2, Vec4},
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        camera::TemporalJitter,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferSize, BufferUsages,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FragmentState, LoadOp,
            Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, ShaderStages, StoreOp, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDimension, TextureViewId, VertexState,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        sync_world::RenderEntity,
        view::{ExtractedView, ViewDepthStencilTexture, ViewTarget},
    },
    shader::Shader,
    utils::default,
};
use render_scene::temporal::{TemporalUpscalerSupport, TemporalUpscaling};

use crate::motion_blur::history::CameraHistory;

#[cfg(feature = "dlss")]
pub(crate) mod dlss;
#[cfg(test)]
#[path = "temporal/tests.rs"]
mod tests;

/// The NGX application identifier Cinnabar registers DLSS under; shared by device creation and
/// the renderer.
#[cfg(feature = "dlss")]
pub const DLSS_PROJECT_ID: uuid::Uuid =
    uuid::Uuid::from_u128(0x1c35_5c5d_6aca_4f96_86dc_e249_a2d6_87dd);

const SHADER: Handle<Shader> = uuid_handle!("5c3b8f16-6a42-4d55-9e0d-71f0c5a2b934");
/// Motion vectors in render pixels, previous minus current position.
pub(crate) const MOTION_FORMAT: TextureFormat = TextureFormat::Rg16Float;

/// Builds camera history and motion vectors for views that request temporal upscaling.
pub struct TemporalUpscalingPlugin;

impl Plugin for TemporalUpscalingPlugin {
    fn build(&self, app: &mut App) {
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        load_internal_asset!(
            app,
            SHADER,
            "temporal/motion_vectors.wgsl",
            crate::shader_safety::from_wgsl
        );
        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(ExtractSchedule, extract_requests)
            .add_systems(
                Render,
                prepare_temporal_views.in_set(RenderSystems::PrepareBindGroups),
            )
            .add_systems(
                Core3d,
                motion_vectors
                    .in_set(Core3dSystems::MainPass)
                    .after(crate::scene_target::ScenePass::Transparent)
                    // Hands zero their own coverage afterwards; see `mask_hand`.
                    .before(crate::viewmodel_render::HandLabel)
                    .before(crate::scene_target::ScenePass::Finish),
            );
        if !render_app
            .world()
            .contains_resource::<TemporalUpscalerSupport>()
        {
            render_app.init_resource::<TemporalUpscalerSupport>();
        }
        #[cfg(feature = "dlss")]
        dlss::install(render_app);
    }
}

type MainCameras<'w, 's> =
    Query<'w, 's, (RenderEntity, Option<&'static TemporalUpscaling>), With<Camera3d>>;

/// Mirrors each camera's request into its render entity, removing it when the camera stops.
fn extract_requests(
    mut commands: Commands,
    cameras: Extract<MainCameras>,
    mut current: Query<&mut TemporalUpscaling>,
) {
    for (entity, desired) in &cameras {
        match (current.get_mut(entity), desired) {
            (Ok(mut current), Some(desired)) => {
                if *current != *desired {
                    *current = *desired;
                }
            }
            (Ok(_), None) => {
                commands.entity(entity).remove::<TemporalUpscaling>();
            }
            (Err(_), Some(desired)) => {
                commands.entity(entity).insert(*desired);
            }
            (Err(_), None) => {}
        }
    }
}

/// Mirrors `Motion` in `motion_vectors.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MotionUniform {
    previous_clip_from_clip: Mat4,
    viewport: Vec4,
}

/// One view's temporal inputs for this frame.
#[derive(bevy::ecs::component::Component)]
pub(crate) struct TemporalView {
    history: CameraHistory,
    texture: Texture,
    view: TextureView,
    uniform: Buffer,
    binding: Option<(TextureViewId, BindGroup)>,
    /// Hand-depth bindings for `mask_hand`, by hand depth view.
    hand_bindings: Mutex<Vec<(TextureViewId, BindGroup)>>,
    /// The previous frame cannot be reprojected; upscalers discard their history.
    pub(crate) reset: bool,
    /// The sub-pixel projection offset in render pixels, as given to Bevy's `TemporalJitter`.
    pub(crate) jitter: Vec2,
}

impl TemporalView {
    /// Motion vectors in render pixels, previous minus current position, without jitter.
    #[cfg(feature = "dlss")]
    pub(crate) fn motion_vectors(&self) -> &TextureView {
        &self.view
    }

    /// The render resolution the inputs were produced at.
    #[cfg(feature = "dlss")]
    pub(crate) fn render_size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }
}

/// The current camera pose of `view`, from the unjittered extracted projection.
fn camera_history(view: &ExtractedView, reset_epoch: u64) -> CameraHistory {
    let pose = view.world_from_view.to_matrix();
    let clip = view.clip_from_world.map_or(view.clip_from_view, |clip| {
        (clip.as_dmat4() * pose.as_dmat4()).as_mat4()
    });
    CameraHistory::new(clip, pose, view.viewport, reset_epoch)
}

/// Whether the previous frame's camera reprojects into this one, and the matrix that does it.
/// Without continuity, motion is zero and the upscaler resets.
fn reprojection(
    previous: Option<CameraHistory>,
    current: CameraHistory,
    request: &TemporalUpscaling,
) -> (bool, Mat4) {
    match previous {
        Some(previous) if previous.camera_continuous(current, request.delta_seconds) => {
            let matrix = previous.previous_clip_from_clip(current);
            if matrix.is_finite() {
                (true, matrix)
            } else {
                (false, Mat4::IDENTITY)
            }
        }
        _ => (false, Mat4::IDENTITY),
    }
}

type TemporalViews<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static TemporalUpscaling,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static ViewDepthStencilTexture,
        Option<&'static TemporalJitter>,
        Option<&'static mut TemporalView>,
    ),
>;

type RemovedViews<'w, 's> = Query<
    'w,
    's,
    Entity,
    (
        With<TemporalView>,
        Or<(Without<TemporalUpscaling>, Without<ViewTarget>)>,
    ),
>;

/// Advances camera history, sizes the motion-vector target to the render resolution and binds
/// this frame's depth.
fn prepare_temporal_views(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    pipeline: Res<MotionVectorPipeline>,
    mut views: TemporalViews,
    removed: RemovedViews,
) {
    for entity in &removed {
        commands.entity(entity).remove::<TemporalView>();
    }
    for (entity, request, view, target, depth, jitter, state) in &mut views {
        let size = target.main_texture().size();
        let current = camera_history(view, request.reset_epoch);
        let jitter = jitter.map_or(Vec2::ZERO, |jitter| jitter.offset);
        let state = match state {
            Some(state)
                if state.texture.width() == size.width && state.texture.height() == size.height =>
            {
                let state = state.into_inner();
                let (continuous, matrix) = reprojection(Some(state.history), current, request);
                state.history = current;
                state.reset = !continuous;
                state.jitter = jitter;
                write_uniform(&queue, &state.uniform, matrix, size);
                state
            }
            _ => {
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("temporal motion vectors"),
                    size: Extent3d {
                        width: size.width,
                        height: size.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: MOTION_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let uniform = device.create_buffer(&BufferDescriptor {
                    label: Some("temporal motion uniform"),
                    size: size_of::<MotionUniform>() as u64,
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                write_uniform(&queue, &uniform, Mat4::IDENTITY, size);
                commands.entity(entity).insert(TemporalView {
                    history: current,
                    view: texture.create_view(&default()),
                    texture,
                    uniform,
                    binding: None,
                    hand_bindings: Mutex::new(Vec::with_capacity(2)),
                    reset: true,
                    jitter,
                });
                continue;
            }
        };
        let depth = crate::scene_sampling::view_depth(depth);
        if state
            .binding
            .as_ref()
            .is_none_or(|(id, _)| *id != depth.id())
        {
            state.binding = Some((
                depth.id(),
                device.create_bind_group(
                    "temporal motion inputs",
                    &cache.get_bind_group_layout(&pipeline.layout),
                    &BindGroupEntries::sequential((depth, state.uniform.as_entire_binding())),
                ),
            ));
        }
    }
}

fn write_uniform(queue: &RenderQueue, uniform: &Buffer, matrix: Mat4, size: Extent3d) {
    queue.write_buffer(
        uniform,
        0,
        bytemuck::bytes_of(&MotionUniform {
            previous_clip_from_clip: matrix,
            viewport: Vec4::new(size.width as f32, size.height as f32, 0.0, 0.0),
        }),
    );
}

/// The motion-vector pass's layout and pipeline.
#[derive(Resource)]
struct MotionVectorPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedRenderPipelineId,
    hand_mask: CachedRenderPipelineId,
}

/// Queues the motion-vector and hand-mask pipelines once the pipeline cache exists.
fn init_pipeline(mut commands: Commands, cache: Res<PipelineCache>) {
    commands.insert_resource(MotionVectorPipeline::new(&cache));
}

impl MotionVectorPipeline {
    fn new(cache: &PipelineCache) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "temporal motion inputs",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Depth,
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
                        min_binding_size: BufferSize::new(size_of::<MotionUniform>() as u64),
                    },
                    count: None,
                },
            ],
        );
        let queue = |label: &'static str, entry: &'static str| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(label.into()),
                layout: vec![layout.clone()],
                vertex: VertexState {
                    shader: SHADER,
                    entry_point: Some("vertex".into()),
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader: SHADER,
                    entry_point: Some(entry.into()),
                    targets: vec![Some(ColorTargetState {
                        format: MOTION_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        };
        let pipeline = queue("temporal motion vectors", "motion_fragment");
        let hand_mask = queue("temporal hand motion mask", "hand_mask_fragment");
        Self {
            layout,
            pipeline,
            hand_mask,
        }
    }
}

/// Writes this frame's motion vectors once the world's depth is final.
fn motion_vectors(
    world: &World,
    view: ViewQuery<Option<&TemporalView>>,
    mut context: RenderContext,
) {
    let Some(state) = view.into_inner() else {
        return;
    };
    let (Some((_, binding)), Some(pipeline)) = (
        state.binding.as_ref(),
        world
            .get_resource::<MotionVectorPipeline>()
            .and_then(|pipeline| {
                world
                    .resource::<PipelineCache>()
                    .get_render_pipeline(pipeline.pipeline)
            }),
    ) else {
        return;
    };
    let attachments = [Some(RenderPassColorAttachment {
        view: &state.view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(default()),
            store: StoreOp::Store,
        },
    })];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("temporal motion vectors"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}

/// Whether `view` feeds a temporal upscaler, so its hand passes keep their depth and call
/// [`mask_hand`].
pub(crate) fn masks_hands(world: &World, view: Entity) -> bool {
    world.get::<TemporalView>(view).is_some()
}

/// Zeroes `view`'s motion vectors wherever `hand_depth` holds a hand drawn this frame: hands
/// move with the camera, while the world motion beneath them does not.
pub(crate) fn mask_hand(
    world: &World,
    context: &mut RenderContext,
    view: Entity,
    hand_depth: &TextureView,
) {
    let (Some(state), Some(pipelines)) = (
        world.get::<TemporalView>(view),
        world.get_resource::<MotionVectorPipeline>(),
    ) else {
        return;
    };
    let cache = world.resource::<PipelineCache>();
    let Some(pipeline) = cache.get_render_pipeline(pipelines.hand_mask) else {
        return;
    };
    if crate::render_bounds::extent(hand_depth) != [state.texture.width(), state.texture.height()] {
        return;
    }
    let binding = {
        let mut bindings = state.hand_bindings.lock().expect("hand mask bindings lock");
        if let Some((_, binding)) = bindings.iter().find(|(id, _)| *id == hand_depth.id()) {
            binding.clone()
        } else {
            // Hand depth targets are reallocated only on resize, so keep the latest two.
            if bindings.len() >= 2 {
                bindings.remove(0);
            }
            let binding = context.render_device().create_bind_group(
                "temporal hand motion mask",
                &cache.get_bind_group_layout(&pipelines.layout),
                &BindGroupEntries::sequential((hand_depth, state.uniform.as_entire_binding())),
            );
            bindings.push((hand_depth.id(), binding.clone()));
            binding
        }
    };
    let attachments = [Some(RenderPassColorAttachment {
        view: &state.view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    })];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("temporal hand motion mask"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &binding, &[]);
    pass.draw(0..3, 0..1);
}

/// Records `view`'s temporal upscaler and returns its output-resolution reconstruction of this
/// frame's world as linear colour, or `None` when no upscaler is ready for the view's current
/// sizes.
pub(crate) fn reconstruct(
    world: &World,
    context: &mut RenderContext,
    view: Entity,
    target: &ViewTarget,
) -> Option<TextureView> {
    #[cfg(feature = "dlss")]
    {
        dlss::evaluate(world, context, view, target)
    }
    #[cfg(not(feature = "dlss"))]
    {
        let _ = (world, context, view, target);
        None
    }
}
