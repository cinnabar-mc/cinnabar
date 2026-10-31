//! Presents only prepared GPU images; real simulation and input schedules run exactly once.
use super::*;
use bevy::{
    camera::NormalizedRenderTarget,
    render::{
        camera::ExtractedCamera,
        render_resource::SurfaceTexture,
        view::window::{ExtractedWindow, SurfaceData},
    },
    window::PrimaryWindow,
};
use render_api::frame_generation::{
    FrameGenerationAdmission, FrameGenerationPacer, FrameGenerationPlan,
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// A single window's drawable remains owned until its real or generated content is presented.
struct Held {
    window: Entity,
    view: Entity,
    surface: SurfaceTexture,
    ready: Arc<gpu::Ready>,
    camera: FrameGenerationCamera,
    interval_ns: u64,
    done: Arc<AtomicBool>,
    outcome: Outcome,
}

/// What `generate` produced for the held real frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    /// No usable candidate: present the real frame now and drop the pacing anchor.
    Ordinary,
    /// History restarted: present the real frame now and anchor the next pair on it.
    Seed,
    /// The surface holds the generated image and the owned copy holds the real one.
    Pair,
}

/// Monotonic presentation state never crosses into the simulation world.
#[derive(Resource)]
pub(super) struct Presenter {
    epoch: Instant,
    pacer: FrameGenerationPacer,
    held: Option<Held>,
}
impl Default for Presenter {
    /// Establishes the edge clock once; deterministic policy receives relative nanoseconds.
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            pacer: FrameGenerationPacer::default(),
            held: None,
        }
    }
}

/// Holds only a primary window that can benefit; captures and unsupported views present normally.
pub(super) fn hold(
    mut presenter: ResMut<Presenter>,
    views: Query<
        (Entity, &ViewState, &FrameGenerationCamera, &ExtractedCamera),
        With<FrameGenerationActive>,
    >,
    mut windows: Query<
        (
            Entity,
            &mut ExtractedWindow,
            &SurfaceData,
            &bevy::render::sync_world::MainEntity,
        ),
        With<PrimaryWindow>,
    >,
) {
    presenter.held = None;
    let Ok((view, state, camera, extracted)) = views.single() else {
        presenter.pacer.reset();
        return;
    };
    let Ok((window, mut output, surface, main)) = windows.single_mut() else {
        presenter.pacer.reset();
        return;
    };
    if !matches!(&extracted.target,Some(NormalizedRenderTarget::Window(target)) if target.entity()==main.id())
        || surface.configuration().width != state.0.key.size.display[0]
        || surface.configuration().height != state.0.key.size.display[1]
    {
        presenter.pacer.reset();
        return;
    }
    let continuous = !state
        .0
        .temporal
        .lock()
        .expect("frame generation history")
        .reset;
    let admission = FrameGenerationAdmission {
        enabled: true,
        prepared: true,
        compatible: true,
        capture: camera.capture,
        continuous,
        interval_ns: camera.interval_ns,
    };
    let interval_ns = match (admission.bypass(), camera.interval_ns) {
        (None, Some(interval)) => interval,
        (reason, _) => {
            presenter.pacer.reset();
            bevy::log::trace!(?reason, "frame generation bypass");
            return;
        }
    };
    if !presenter.pacer.admit() {
        bevy::log::trace!("frame generation backing off after a missed deadline");
        return;
    }
    if let Some(surface) = output.swap_chain_texture.take() {
        presenter.held = Some(Held {
            window,
            view,
            surface,
            ready: Arc::clone(&state.0),
            camera: *camera,
            interval_ns,
            done: Arc::new(AtomicBool::new(false)),
            outcome: Outcome::Ordinary,
        });
    }
}

/// Encodes world interpolation and a UI-only replay after all real-frame producers were submitted.
pub(super) fn generate(world: &mut World) {
    world.resource_mut::<crate::device_poll::FrameWait>().0 = None;
    let mut presenter = world
        .remove_resource::<Presenter>()
        .expect("frame generation presenter");
    let Some(held) = presenter.held.as_mut() else {
        // A bypassed interval must never bridge the next pair.
        let mut query = world.query::<&ViewState>();
        for state in query.iter(world) {
            let mut temporal = state.0.temporal.lock().expect("frame generation history");
            temporal.history = temporal.current;
            temporal.generator.invalidate();
        }
        world.insert_resource(presenter);
        return;
    };
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let complete = composition_ready(world, held.view);
    let generated = {
        let mut temporal = held
            .ready
            .temporal
            .lock()
            .expect("frame generation history");
        if !complete || !temporal.world_captured || !temporal.real_captured {
            temporal.generator.invalidate();
            None
        } else {
            let projection = temporal
                .history
                .zip(temporal.current)
                .map_or(Mat4::IDENTITY, |(old, current)| old.reprojection(current));
            let frame = FrameGenerationInput {
                color: &held.ready.world,
                depth: &held.ready.depth._texture,
                previous_clip_from_clip: projection,
                jitter: [0.0; 2],
                near: held.camera.near,
                far: held.camera.far,
                fov_y: held.camera.fov_y,
                reset: temporal.reset,
            };
            let result = temporal
                .generator
                .generate(device.wgpu_device(), &queue, &frame)
                .ok()
                .map(|image| (image.image.clone(), image.history_reset));
            temporal.history = temporal.current;
            result
        }
    };
    held.outcome = match generated {
        None => Outcome::Ordinary,
        Some((_, true)) => Outcome::Seed,
        Some((image, false)) => 'replay: {
            let Some(native) = world.get::<crate::ui_render::target::NativeUiTarget>(held.view)
            else {
                break 'replay Outcome::Ordinary;
            };
            let destination = native.replace_scene();
            let mut encoder = device.create_command_encoder(&Default::default());
            held.ready.blit(
                device.wgpu_device(),
                &mut encoder,
                &held.ready.copy_generated,
                &image,
                destination,
            );
            queue.submit([encoder.finish()]);
            world.resource_mut::<Replay>().0 = Some(held.view);
            world.insert_resource(bevy::render::renderer::CurrentView(held.view));
            world.run_schedule(GeneratedUi);
            world.resource_mut::<Replay>().0 = None;
            if composition_ready(world, held.view) {
                Outcome::Pair
            } else {
                // The surface may hold a partial replay; present restores the real image.
                Outcome::Ordinary
            }
        }
    };
    if held.outcome == Outcome::Pair {
        let done = Arc::clone(&held.done);
        queue.on_submitted_work_done(move || done.store(true, Ordering::Release));
        world.resource_mut::<crate::device_poll::FrameWait>().0 =
            Some(Duration::from_nanos(held.interval_ns));
    }
    world.insert_resource(presenter);
}

/// A second presentation requires the same complete UI publication as the real output.
fn composition_ready(world: &World, entity: Entity) -> bool {
    world
        .get::<crate::ui_render::target::NativeUiTarget>(entity)
        .is_some_and(|native| native.is_ready() && native.was_presented())
        && (world
            .get_resource::<crate::ui_render::UiGpu>()
            .is_none_or(|gpu| gpu.batches.is_empty())
            || world
                .get::<crate::ui_render::composite::UiLayerTexture>(entity)
                .is_some_and(|layer| layer.presented_complete_content()))
}

/// Restores a complete real image whenever no timely midpoint can be presented.
fn restore(
    ready: &gpu::Ready,
    device: &RenderDevice,
    queue: &RenderQueue,
    target: &wgpu::TextureView,
) {
    let mut encoder = device.create_command_encoder(&Default::default());
    ready.blit(
        device.wgpu_device(),
        &mut encoder,
        &ready.present,
        &ready.real_view,
        target,
    );
    queue.submit([encoder.finish()]);
}

/// Waits on absolute OS deadlines, then presents the held midpoint and its real frame.
pub(super) fn present(world: &mut World) {
    let mut presenter = world
        .remove_resource::<Presenter>()
        .expect("frame generation presenter");
    let Some(held) = presenter.held.take() else {
        world.insert_resource(presenter);
        return;
    };
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let epoch = presenter.epoch;
    let now = || u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let deadline = |at_ns: u64| epoch + Duration::from_nanos(at_ns);
    let target = world
        .get::<ExtractedWindow>(held.window)
        .and_then(|window| window.swap_chain_texture_view.clone());
    let plan = match held.outcome {
        Outcome::Ordinary => None,
        Outcome::Seed => Some(FrameGenerationPlan::Warmup),
        Outcome::Pair if !held.done.load(Ordering::Acquire) => {
            presenter.pacer.missed();
            None
        }
        Outcome::Pair => Some(presenter.pacer.plan(now(), held.interval_ns)),
    };
    match (plan, &target) {
        (Some(FrameGenerationPlan::Pair { generated_ns, .. }), Some(_)) => {
            crate::input_pacing::wait::sleep_until(deadline(generated_ns));
            held.surface.present(&queue);
            // A late wake or slow present moves the real deadline so the midpoint holds.
            let real_deadline = presenter.pacer.real_deadline(now());
            let real = world
                .get::<SurfaceData>(held.window)
                .map(|surface| surface.surface().get_current_texture());
            match real {
                Some(
                    wgpu::CurrentSurfaceTexture::Success(real)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(real),
                ) => {
                    let view = real.texture.create_view(&wgpu::TextureViewDescriptor {
                        format: Some(held.ready.key.output),
                        ..Default::default()
                    });
                    restore(&held.ready, &device, &queue, &view);
                    if let Some(real_ns) = real_deadline {
                        crate::input_pacing::wait::sleep_until(deadline(real_ns));
                    }
                    queue.present(real);
                    presenter.pacer.presented_real(now());
                    bevy::log::trace!(generated_ns, ?real_deadline, "frame generation pair");
                }
                // The real image is lost with the surface; the next frame starts over.
                _ => invalidate(&held.ready, &mut presenter),
            }
        }
        (Some(FrameGenerationPlan::Warmup), Some(target)) => {
            if held.outcome == Outcome::Pair {
                restore(&held.ready, &device, &queue, target);
            }
            held.surface.present(&queue);
            presenter.pacer.presented_real(now());
        }
        (plan, Some(target)) => {
            bevy::log::trace!(outcome = ?held.outcome, ?plan, "frame generation real only");
            restore_captured(&held.ready, &device, &queue, target);
            held.surface.present(&queue);
            invalidate(&held.ready, &mut presenter);
        }
        (_, None) => invalidate(&held.ready, &mut presenter),
    }
    world
        .resource_mut::<crate::device_poll::FramesInFlight>()
        .track_tail(&queue);
    world.insert_resource(presenter);
}

/// A partial UI replay may have overwritten the surface; the captured real image replaces it.
fn restore_captured(
    ready: &gpu::Ready,
    device: &RenderDevice,
    queue: &RenderQueue,
    target: &wgpu::TextureView,
) {
    let captured = ready
        .temporal
        .lock()
        .expect("frame generation history")
        .real_captured;
    if captured {
        restore(ready, device, queue, target);
    }
}

/// Drops the pacing anchor and image history so no later pair bridges this frame.
fn invalidate(ready: &gpu::Ready, presenter: &mut Presenter) {
    presenter.pacer.reset();
    ready
        .temporal
        .lock()
        .expect("frame generation history")
        .generator
        .invalidate();
}
