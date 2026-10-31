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
    done: Arc<AtomicBool>,
    generated: bool,
    complete: bool,
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
        synchronized_refresh_ns: camera.synchronized_refresh_ns,
    };
    if let Some(reason) = admission.bypass() {
        presenter.pacer.reset();
        bevy::log::trace!(?reason, "frame generation bypass");
        return;
    }
    if let Some(surface) = output.swap_chain_texture.take() {
        presenter.held = Some(Held {
            window,
            view,
            surface,
            ready: Arc::clone(&state.0),
            camera: *camera,
            done: Arc::new(AtomicBool::new(false)),
            generated: false,
            complete: true,
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
    if let Some((image, reset)) = generated {
        if !reset
            && let Some(native) = world.get::<crate::ui_render::target::NativeUiTarget>(held.view)
        {
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
            held.generated = true;
            held.complete = composition_ready(world, held.view);
        }
        if !held.complete {
            presenter.pacer.reset();
            world.insert_resource(presenter);
            return;
        }
        let done = Arc::clone(&held.done);
        queue.on_submitted_work_done(move || done.store(true, Ordering::Release));
        world.resource_mut::<crate::device_poll::FrameWait>().0 =
            Some(Duration::from_nanos(held.camera.interval_ns));
    } else {
        presenter.pacer.reset();
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
    let plan = if held.done.load(Ordering::Acquire) {
        presenter.pacer.plan(now(), held.camera.interval_ns)
    } else {
        presenter.pacer.reset();
        FrameGenerationPlan::Bypass(
            render_api::frame_generation::FrameGenerationBypass::MissedDeadline,
        )
    };
    let target = world
        .get::<ExtractedWindow>(held.window)
        .and_then(|window| window.swap_chain_texture_view.clone());
    if let Some(target) = &target {
        match plan {
            FrameGenerationPlan::Pair {
                generated_ns,
                real_ns: _,
            } if held.generated => {
                crate::input_pacing::wait::sleep_until(
                    presenter.epoch + Duration::from_nanos(generated_ns),
                );
                held.surface.present(&queue);
                let actual_generated = now();
                // Account for a delayed wake or present before choosing the real deadline.
                let real_deadline = match presenter
                    .pacer
                    .plan(actual_generated, held.camera.interval_ns)
                {
                    FrameGenerationPlan::Pair { real_ns, .. } => Some(real_ns),
                    _ => None,
                };
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
                            crate::input_pacing::wait::sleep_until(
                                presenter.epoch + Duration::from_nanos(real_ns),
                            );
                        }
                        queue.present(real);
                        if real_deadline.is_some() {
                            presenter.pacer.presented_real(now());
                        }
                        bevy::log::trace!(
                            actual_generated,
                            ?real_deadline,
                            "frame generation pair presented"
                        );
                    }
                    _ => presenter.pacer.reset(),
                }
            }
            FrameGenerationPlan::Warmup { real_ns } => {
                if held.generated {
                    restore(&held.ready, &device, &queue, target);
                }
                crate::input_pacing::wait::sleep_until(
                    presenter.epoch + Duration::from_nanos(real_ns),
                );
                held.surface.present(&queue);
                presenter.pacer.presented_real(now());
            }
            _ => {
                if held.generated {
                    restore(&held.ready, &device, &queue, target);
                }
                held.surface.present(&queue);
                presenter.pacer.reset();
                held.ready
                    .temporal
                    .lock()
                    .expect("frame generation history")
                    .generator
                    .invalidate();
            }
        }
    } else {
        presenter.pacer.reset();
    }
    world
        .resource_mut::<crate::device_poll::FramesInFlight>()
        .track_tail(&queue);
    world.insert_resource(presenter);
}
