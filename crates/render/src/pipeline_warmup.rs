//! Queues first-use pipeline variants as soon as a view exists, so loading absorbs compilation.

use bevy::prelude::IntoScheduleConfigs;
use render_scene::{PipelineWarmupReadiness, SampleCountPreflight};

use std::collections::HashSet;

#[path = "pipeline_warmup/progress.rs"]
mod progress;
use progress::{WarmupCounts, WarmupProgress};

#[cfg(test)]
use bevy::prelude::{Handle, Mut, Shader, World, default};
use bevy::{
    prelude::{
        App, BevyError, Camera3d, Has, Local, Msaa, Query, Res, ResMut, Resource, Result, SubApp,
        SystemSet, error, info,
    },
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        render_resource::{
            CachedPipelineState, CachedRenderPipelineId, PipelineCache, TextureFormat,
        },
        view::ViewTarget,
    },
};

/// The view properties our built-in pipeline keys depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    requested: Option<Msaa>,
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
enum WarmupSet {
    Views,
    Owners,
}

#[derive(Resource)]
struct WarmupOwner<T>(std::marker::PhantomData<T>);

/// Idempotent, so plugins that install from both `build` and `finish` may call it twice.
pub(crate) fn register<T: PrewarmPipelines>(app: &mut App) {
    let Some(render_app) = install(app) else {
        return;
    };
    if !render_app.world().contains_resource::<WarmupOwner<T>>() {
        render_app
            .insert_resource(WarmupOwner::<T>(std::marker::PhantomData))
            .add_systems(
                Render,
                prewarm_owner::<T>
                    .in_set(WarmupSet::Owners)
                    .run_if(render_scene::startup_work::world_work_allowed),
            );
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
                    .run_if(render_scene::startup_work::world_work_allowed)
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
    app.init_resource::<render_scene::startup_work::WorldWorkAdmission>();
    let admission = app
        .world()
        .resource::<render_scene::startup_work::WorldWorkAdmission>()
        .clone();
    let shared = app.world().resource::<PipelineWarmupReadiness>().clone();
    let render_app = app.get_sub_app_mut(RenderApp)?;
    render_app.insert_resource(admission);
    crate::scene_target::install_formats(render_app);
    if !render_app.world().contains_resource::<WarmupRegistry>() {
        render_app
            .insert_resource(shared)
            .insert_resource(preflight)
            .init_resource::<WarmupRegistry>()
            // Bevy's upscaling blocks on its blit pipeline in `Prepare`, and that wait starts
            // every queued pipeline first; their tasks share one shader-cache lock. Queuing
            // after it lets the blit compile alone, and this frame's `Render` set still starts
            // the warm-up.
            .configure_sets(
                Render,
                (WarmupSet::Views, WarmupSet::Owners)
                    .chain()
                    .after(RenderSystems::Prepare)
                    .before(RenderSystems::Render),
            )
            .add_systems(ExtractSchedule, extract_sample_request)
            .add_systems(Render, collect_views.in_set(WarmupSet::Views))
            .add_systems(Render, publish_readiness.in_set(RenderSystems::Cleanup));
    }
    Some(render_app)
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
    mut registry: ResMut<WarmupRegistry>,
) {
    registry.views.clear();
    registry.pending = false;
    registry.requested = None;
    for (camera, msaa, target, enhanced, shader_pack, world_camera) in &views {
        let key = WarmView {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: enhanced.is_some(),
            shader_pack,
            output: target.and_then(ViewTarget::out_texture_view_format),
        };
        let requested = requested_sample_mode(preflight.requested(), world_camera, shader_pack);
        registry.requested = registry.requested.or(requested);
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
    mut warmed: Local<Vec<WarmView>>,
) {
    // An owner that never initialised has nothing it could draw.
    let Some(mut owner) = owner else {
        return;
    };
    let WarmupRegistry { views, ids, .. } = &mut *registry;
    for &view in views.iter() {
        for msaa in sample_modes(view.msaa) {
            let view = WarmView { msaa, ..view };
            if warmed.contains(&view) {
                continue;
            }
            // The draw path would fail the same way, so the error does not hold loading.
            if let Err(error) = owner.prewarm(&cache, view, ids) {
                error!("pipeline prewarm {}: {error}", std::any::type_name::<T>());
            }
            warmed.push(view);
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
    admission: Option<Res<render_scene::startup_work::WorldWorkAdmission>>,
    mut progress: Local<WarmupProgress>,
) {
    let ready = registered_pipelines_ready(&cache, &mut registry);
    shared.set_ready(ready);
    // Owners only queue variants once world work is admitted; before that, an empty set proves nothing.
    let admitted = render_scene::startup_work::world_work_allowed(admission);
    preflight.publish_ready(registry.requested.filter(|_| ready && admitted));
    if progress.wants_counts(ready) {
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
