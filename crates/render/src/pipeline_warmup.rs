//! Queues first-use pipeline variants as soon as a view exists, so loading absorbs compilation.
//!
//! The interface's variants go first: the output blit Bevy's upscaling waits for on the render
//! thread, then the UI and menu owners. World owners queue once those have settled, so nothing
//! the menu or a loading screen draws with waits behind the world's compiles.

use bevy::prelude::IntoScheduleConfigs;
use render_scene::{PipelineWarmupReadiness, SampleCountPreflight, SampleCountRequest};

use std::collections::{HashMap, HashSet};

#[path = "pipeline_warmup/progress.rs"]
mod progress;
use progress::{WarmupCounts, WarmupProgress};

#[cfg(test)]
use bevy::prelude::{Handle, Mut, Shader, World, default};
use bevy::{
    camera::CameraOutputMode,
    core_pipeline::blit::{BlitPipeline, BlitPipelineKey},
    render::render_resource::BlendState,
};
use bevy::{
    prelude::{
        App, BevyError, Camera3d, Has, Local, Msaa, Query, Res, ResMut, Resource, Result, SubApp,
        SystemSet, error, info,
    },
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        render_resource::{
            CachedPipelineState, CachedRenderPipelineId, PipelineCache, SpecializedRenderPipelines,
            TextureFormat,
        },
        view::{ResolvedCompositingSpace, ViewTarget},
    },
};
use render_scene::startup_work::{WorldWorkAdmission, world_work_allowed};

/// The view properties our built-in pipeline keys depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WarmView {
    pub msaa: Msaa,
    pub hdr: bool,
    pub enhanced: bool,
    pub shader_pack: bool,
    /// The surface or image format the view finally writes; absent until its target exists.
    pub output: Option<TextureFormat>,
}

pub(crate) type WarmupIds = Vec<CachedRenderPipelineId>;

/// Enumerates active coverage and Performance's single-sample path without duplicates.
pub(crate) fn sample_modes(current: Msaa) -> impl Iterator<Item = Msaa> {
    [Some(current), (current != Msaa::Off).then_some(Msaa::Off)]
        .into_iter()
        .flatten()
}

/// Implemented by each pipeline owner so warmup fills the same memoized variants drawing uses.
pub(crate) trait PrewarmPipelines:
    Resource<Mutability = bevy::ecs::component::Mutable>
{
    /// Pushes the ID of every variant a draw for `view` may later request.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError>;
}

/// Implemented by owners that compile outside the pipeline cache, as their inputs appear.
pub(crate) trait PendingPipelines: Resource {
    /// Whether a pipeline a draw may request this frame is still compiling.
    fn pending(&self) -> bool;
}

#[derive(Resource, Default)]
struct WarmupRegistry {
    views: Vec<WarmView>,
    ids: WarmupIds,
    failed: HashSet<CachedRenderPipelineId>,
    /// Owned compiles still running this frame.
    pending: bool,
    /// A requested sample count this frame's world-camera views also warm.
    requested: Option<SampleCountRequest>,
    /// Whether shader work was requested when this frame's views were collected. The whole
    /// interface stage reads this one sample, so a request arriving mid-frame cannot admit
    /// world owners past interface owners that did not run.
    interface_open: bool,
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
enum WarmupSet {
    Views,
    /// Output blits and the owners registered with [`register_interface`].
    Interface,
    /// World owners, which queue only after the interface's variants have settled.
    Owners,
}

#[derive(Resource)]
struct WarmupOwner<T>(std::marker::PhantomData<T>);

/// Registers a world owner. Idempotent, so plugins that install from both `build` and `finish`
/// may call it twice.
pub(crate) fn register<T: PrewarmPipelines>(app: &mut App) {
    register_in::<T>(app, WarmupSet::Owners);
}

/// Registers an owner the menu, a loading screen or the HUD draws with, whose variants compile
/// ahead of every world owner's. Idempotent like [`register`].
pub(crate) fn register_interface<T: PrewarmPipelines>(app: &mut App) {
    register_in::<T>(app, WarmupSet::Interface);
}

/// Adds `T`'s warm-up system once, behind the admission its stage waits for.
fn register_in<T: PrewarmPipelines>(app: &mut App, stage: WarmupSet) {
    let Some(render_app) = install(app) else {
        return;
    };
    if render_app.world().contains_resource::<WarmupOwner<T>>() {
        return;
    }
    render_app.insert_resource(WarmupOwner::<T>(std::marker::PhantomData));
    let interface = stage == WarmupSet::Interface;
    let system = prewarm_owner::<T>.in_set(stage);
    if interface {
        render_app.add_systems(Render, system.run_if(interface_stage_open));
    } else {
        render_app.add_systems(Render, system.run_if(world_work_allowed));
    }
}

/// Holds readiness while `T` reports a compile in flight; idempotent like [`register`].
pub(crate) fn register_pending<T: PendingPipelines>(app: &mut App) {
    let Some(render_app) = install(app) else {
        return;
    };
    if !render_app.world().contains_resource::<WarmupOwner<T>>() {
        render_app
            .insert_resource(WarmupOwner::<T>(std::marker::PhantomData))
            .add_systems(
                Render,
                owner_pending::<T>
                    .run_if(world_work_allowed)
                    .in_set(RenderSystems::Cleanup)
                    .before(publish_readiness),
            );
    }
}

/// Holds world-entry readiness while an asynchronous GPU owner is not yet installed.
pub(crate) fn register_pending_resource<T: Resource>(app: &mut SubApp) {
    app.init_resource::<WarmupRegistry>();
    app.add_systems(
        Render,
        pending_resource::<T>
            .in_set(RenderSystems::Cleanup)
            .before(publish_readiness),
    );
}

/// Folds the existence of an unfinished initializer into the same pipeline-readiness gate.
fn pending_resource<T: Resource>(pending: Option<Res<T>>, mut registry: ResMut<WarmupRegistry>) {
    registry.pending |= pending.is_some();
}

/// Installs shared scene formats and readiness tracking for custom pipeline owners.
fn install(app: &mut App) -> Option<&mut SubApp> {
    app.init_resource::<PipelineWarmupReadiness>();
    app.init_resource::<SampleCountPreflight>();
    let preflight = app.world().resource::<SampleCountPreflight>().clone();
    app.init_resource::<WorldWorkAdmission>();
    let admission = app.world().resource::<WorldWorkAdmission>().clone();
    let shared = app.world().resource::<PipelineWarmupReadiness>().clone();
    let render_app = app.get_sub_app_mut(RenderApp)?;
    crate::scene_target::install_formats(render_app);
    if !render_app.world().contains_resource::<InterfaceFirst>() {
        // The systems below release this hold, so it is taken only where they are installed.
        admission.hold_for_interface();
        render_app
            .insert_resource(InterfaceFirst)
            .insert_resource(admission)
            .insert_resource(shared)
            .insert_resource(preflight)
            .init_resource::<WarmupRegistry>()
            // Bevy's upscaling blocks on its blit pipeline in `Prepare`, and that wait starts
            // every queued pipeline first; their tasks share one shader-cache lock. Queuing
            // after it lets the blit compile alone, and this frame's `Render` set still starts
            // the warm-up.
            .configure_sets(
                Render,
                (WarmupSet::Views, WarmupSet::Interface, WarmupSet::Owners)
                    .chain()
                    .after(RenderSystems::Prepare)
                    .before(RenderSystems::Render),
            )
            .add_systems(ExtractSchedule, extract_sample_request)
            .add_systems(Render, collect_views.in_set(WarmupSet::Views))
            .add_systems(
                Render,
                (
                    track_output_blits
                        .in_set(WarmupSet::Interface)
                        .run_if(interface_stage_open),
                    admit_world_after_interface
                        .after(WarmupSet::Interface)
                        .before(WarmupSet::Owners),
                ),
            )
            .add_systems(Render, publish_readiness.in_set(RenderSystems::Cleanup));
    }
    Some(render_app)
}

/// Whether this frame warms the interface's own variants.
fn interface_stage_open(registry: Res<WarmupRegistry>) -> bool {
    registry.interface_open
}

/// Marks a render app whose warm-up systems are installed. The registry cannot mark it,
/// because a deferred GPU owner may create that first.
#[derive(Resource)]
struct InterfaceFirst;

/// The blit variant Bevy's upscaling requests for a view's output, and then waits for on the
/// render thread.
pub(crate) fn output_blit_key(
    target: &ViewTarget,
    camera: Option<&ExtractedCamera>,
    space: Option<&ResolvedCompositingSpace>,
) -> Option<BlitPipelineKey> {
    let blend_state = match camera.map(|camera| &camera.output_mode) {
        Some(CameraOutputMode::Write { blend_state, .. }) => blend_state.or_else(|| {
            camera
                .filter(|camera| camera.sorted_camera_index_for_target > 0)
                .map(|_| BlendState::ALPHA_BLENDING)
        }),
        Some(CameraOutputMode::Skip) | None => None,
    };
    Some(BlitPipelineKey {
        target_format: target.out_texture_view_format()?,
        blend_state,
        samples: 1,
        source_space: ResolvedCompositingSpace::space(space),
    })
}

/// Tracks every view's output blit as an interface variant. Upscaling requested the same key
/// earlier this frame, so this compiles nothing new; it keeps world owners from queuing while
/// a blit is still unsettled, for instance after its shader loaded late.
fn track_output_blits(
    views: Query<(
        &ViewTarget,
        Option<&ExtractedCamera>,
        Option<&ResolvedCompositingSpace>,
    )>,
    cache: Res<PipelineCache>,
    blit: Option<Res<BlitPipeline>>,
    pipelines: Option<ResMut<SpecializedRenderPipelines<BlitPipeline>>>,
    mut registry: ResMut<WarmupRegistry>,
) {
    let (Some(blit), Some(mut pipelines)) = (blit, pipelines) else {
        return;
    };
    for (target, camera, space) in &views {
        if let Some(key) = output_blit_key(target, camera, space) {
            let id = pipelines.specialize(&cache, &blit, key);
            if !registry.ids.contains(&id) {
                registry.ids.push(id);
            }
        }
    }
}

/// Admits world shader work once every interface variant for the current views has settled.
/// The interface stage has just run, so the registry holds exactly its variants.
fn admit_world_after_interface(
    cache: Res<PipelineCache>,
    registry: Res<WarmupRegistry>,
    admission: Res<WorldWorkAdmission>,
) {
    if registry.interface_open
        && !admission.is_admitted()
        && !registry.views.is_empty()
        && registry
            .ids
            .iter()
            .all(|&id| progress::settled(cache.get_render_pipeline_state(id)))
    {
        admission.interface_settled();
    }
}

/// The camera state that selects each view's pipeline variants.
type ViewKeys<'w, 's> = Query<
    'w,
    's,
    (
        &'static bevy::render::camera::ExtractedCamera,
        &'static Msaa,
        Option<&'static ViewTarget>,
        Option<&'static render_scene::EnhancedRendering>,
        Has<render_scene::shaderpack::ShaderPackRendering>,
        Has<Camera3d>,
    ),
>;

/// Copies the main world's pending sample count, so every render system this frame sees one value.
fn extract_sample_request(
    main: Extract<Res<SampleCountPreflight>>,
    mut render: ResMut<SampleCountPreflight>,
) {
    if render.requested() != main.requested() {
        render.request(main.requested());
    }
}

/// The sample counts a view needs ready: its own, plus a requested one it is about to switch to.
/// Shader-pack views stay single-sampled, and other cameras never take the world's setting.
pub(crate) fn requested_sample_mode(
    requested: Option<Msaa>,
    world_camera: bool,
    shader_pack: bool,
) -> Option<Msaa> {
    requested.filter(|_| world_camera && !shader_pack)
}

fn collect_views(
    views: ViewKeys,
    preflight: Res<SampleCountPreflight>,
    admission: Option<Res<WorldWorkAdmission>>,
    mut registry: ResMut<WarmupRegistry>,
) {
    registry.views.clear();
    registry.ids.clear();
    registry.pending = false;
    registry.requested = None;
    registry.interface_open = admission.is_none_or(|admission| admission.shaders_requested());
    for (camera, msaa, target, enhanced, shader_pack, world_camera) in &views {
        let key = WarmView {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: enhanced.is_some(),
            shader_pack,
            output: target.and_then(ViewTarget::out_texture_view_format),
        };
        let request = preflight.requested();
        let requested = requested_sample_mode(
            request.and_then(|request| request.for_view(camera.hdr, enhanced.is_some())),
            world_camera,
            shader_pack,
        );
        if requested.is_some() {
            registry.requested = request;
        }
        for key in [Some(key), requested.map(|msaa| WarmView { msaa, ..key })]
            .into_iter()
            .flatten()
        {
            if !registry.views.contains(&key) {
                registry.views.push(key);
            }
        }
    }
}

/// Specializes once per view configuration; later frames only compare keys.
fn prewarm_owner<T: PrewarmPipelines>(
    owner: Option<ResMut<T>>,
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
    mut warmed: Local<HashMap<WarmView, WarmupIds>>,
) {
    // An owner that never initialised has nothing it could draw.
    let Some(mut owner) = owner else {
        return;
    };
    let WarmupRegistry { views, ids, .. } = &mut *registry;
    for &view in views.iter() {
        for msaa in sample_modes(view.msaa) {
            let view = WarmView { msaa, ..view };
            let view_ids = warmed.entry(view).or_insert_with(|| {
                let mut view_ids = Vec::new();
                // The draw path would fail the same way, so the error does not hold loading.
                if let Err(error) = owner.prewarm(&cache, view, &mut view_ids) {
                    error!("pipeline prewarm {}: {error}", std::any::type_name::<T>());
                }
                view_ids
            });
            ids.extend_from_slice(view_ids);
        }
    }
}

fn owner_pending<T: PendingPipelines>(owner: Option<Res<T>>, mut registry: ResMut<WarmupRegistry>) {
    registry.pending |= owner.is_some_and(|owner| owner.pending());
}

fn publish_readiness(
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
    shared: Res<PipelineWarmupReadiness>,
    preflight: Res<SampleCountPreflight>,
    admission: Option<Res<WorldWorkAdmission>>,
    mut progress: Local<WarmupProgress>,
) {
    // World owners only queue variants once their work is admitted; before that, a settled
    // set proves nothing about the world.
    let admitted = admission
        .as_ref()
        .is_none_or(|admission| admission.is_admitted());
    let ready = registered_pipelines_ready(&cache, &mut registry) && admitted;
    shared.set_ready(ready);
    // While shader work waits for a join, no world has been drawn and nothing is warmed for
    // the current count either, so a new count switches at once instead of doubling the join.
    let awaits_join = admission.is_some_and(|admission| admission.awaits_join());
    preflight.publish_ready(registry.requested.filter(|_| ready || awaits_join));
    if admitted && progress.wants_counts(ready) {
        let counts = WarmupCounts::read(&cache, &registry.ids, registry.pending);
        let workers =
            bevy::tasks::AsyncComputeTaskPool::try_get().map_or(0, |pool| pool.thread_num());
        if let Some(line) = progress.observe(&counts, workers, web_time::Instant::now()) {
            info!("{line}");
        }
    }
}

/// Shader loads keep loading held; terminal compile errors are logged once and stop blocking.
fn registered_pipelines_ready(cache: &PipelineCache, registry: &mut WarmupRegistry) -> bool {
    let WarmupRegistry {
        views,
        ids,
        failed,
        pending,
        ..
    } = registry;
    let mut ready = !views.is_empty() && !*pending;
    for &id in ids.iter() {
        let state = cache.get_render_pipeline_state(id);
        if let CachedPipelineState::Err(error) = state
            && !progress::retryable(error)
            && failed.insert(id)
        {
            error!("pipeline prewarm {id:?}: {error}");
        }
        ready &= progress::settled(state);
    }
    ready
}

#[cfg(test)]
#[path = "pipeline_warmup/tests.rs"]
mod tests;
