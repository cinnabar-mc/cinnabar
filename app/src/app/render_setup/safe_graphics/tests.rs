use super::*;

/// A frame of a run that has presented and has no GPU failure.
fn frame(phase: SessionPhase, warming: bool) -> Frame {
    Frame {
        presented: true,
        phase,
        warming,
        gpu_failed: false,
        panicked: false,
    }
}

/// Feeds `frame` for `time` of real time in steps a frame may be credited with.
fn run_for(tracker: &mut RunTracker, frame: Frame, time: Duration) -> Vec<Changes> {
    let frames = time.as_millis() / MAX_FRAME_CREDIT.as_millis();
    (0..frames)
        .map(|_| tracker.observe(frame, MAX_FRAME_CREDIT))
        .filter(|changes| *changes != Changes::default())
        .collect()
}

const FRAME: Duration = Duration::from_millis(16);

#[test]
fn stages_follow_presentation_session_phase_and_world_warm_up() {
    for phase in [
        SessionPhase::Menus,
        SessionPhase::Joining,
        SessionPhase::InWorld,
    ] {
        for warming in [false, true] {
            let unpresented = Frame {
                presented: false,
                ..frame(phase, warming)
            };
            assert_eq!(stage(unpresented), RunStage::Startup);
        }
    }
    assert_eq!(stage(frame(SessionPhase::Menus, false)), RunStage::Menu);
    assert_eq!(stage(frame(SessionPhase::Menus, true)), RunStage::WarmUp);
    // Compiling during a join or in a world belongs to that stage.
    for warming in [false, true] {
        assert_eq!(
            stage(frame(SessionPhase::Joining, warming)),
            RunStage::Joining
        );
        assert_eq!(
            stage(frame(SessionPhase::InWorld, warming)),
            RunStage::InWorld
        );
    }
}

#[test]
fn the_marker_changes_on_transitions_and_never_on_a_steady_frame() {
    let mut tracker = RunTracker::default();
    let startup = Frame {
        presented: false,
        ..frame(SessionPhase::Menus, false)
    };
    // The run already begins at startup, so its frames record nothing.
    for _ in 0..3 {
        assert_eq!(tracker.observe(startup, FRAME), Changes::default());
    }
    let script = [
        (frame(SessionPhase::Menus, true), RunStage::WarmUp),
        (frame(SessionPhase::Menus, false), RunStage::Menu),
        (frame(SessionPhase::Joining, true), RunStage::Joining),
        (frame(SessionPhase::InWorld, false), RunStage::InWorld),
        (frame(SessionPhase::Menus, false), RunStage::Menu),
    ];
    for (frame, stage) in script {
        assert_eq!(
            tracker.observe(frame, FRAME),
            Changes {
                stage: Some(stage),
                ..Changes::default()
            }
        );
        for _ in 0..100 {
            assert_eq!(
                tracker.observe(frame, FRAME),
                Changes::default(),
                "{stage:?}"
            );
        }
    }
}

#[test]
fn a_session_is_healthy_once_after_enough_time_in_a_world() {
    let mut tracker = RunTracker::default();
    let menu = frame(SessionPhase::Menus, false);
    let world = frame(SessionPhase::InWorld, false);
    let healthy = Changes {
        healthy: true,
        ..Changes::default()
    };
    // Time in the menu, however long, proves nothing about world rendering.
    tracker.observe(menu, FRAME);
    assert!(run_for(&mut tracker, menu, HEALTHY_WORLD_TIME * 3).is_empty());
    tracker.observe(world, Duration::ZERO);
    let half = HEALTHY_WORLD_TIME / 2;
    assert!(run_for(&mut tracker, world, half).is_empty());
    // Leaving the world pauses the count without resetting it.
    tracker.observe(menu, FRAME);
    assert!(run_for(&mut tracker, menu, HEALTHY_WORLD_TIME).is_empty());
    tracker.observe(world, Duration::ZERO);
    assert!(run_for(&mut tracker, world, half - MAX_FRAME_CREDIT).is_empty());
    assert_eq!(tracker.observe(world, MAX_FRAME_CREDIT), healthy);
    assert!(run_for(&mut tracker, world, HEALTHY_WORLD_TIME * 2).is_empty());
}

#[test]
fn time_the_app_spent_suspended_is_not_time_in_a_world() {
    let mut tracker = RunTracker::default();
    let world = frame(SessionPhase::InWorld, false);
    tracker.observe(world, FRAME);
    // The first frame after a resume reports the whole suspension as its duration.
    assert_eq!(
        tracker.observe(world, HEALTHY_WORLD_TIME * 100),
        Changes::default()
    );
    let needed = HEALTHY_WORLD_TIME - MAX_FRAME_CREDIT * 2;
    assert!(run_for(&mut tracker, world, needed).is_empty());
    assert!(tracker.observe(world, MAX_FRAME_CREDIT).healthy);
}

#[test]
fn a_gpu_failure_is_recorded_once_and_stops_the_healthy_count() {
    let mut tracker = RunTracker::default();
    let world = frame(SessionPhase::InWorld, false);
    tracker.observe(world, Duration::ZERO);
    assert!(run_for(&mut tracker, world, HEALTHY_WORLD_TIME / 2).is_empty());
    let failed = Frame {
        gpu_failed: true,
        ..world
    };
    let first = tracker.observe(failed, FRAME);
    assert!(first.gpu_failed && !first.healthy);
    assert!(run_for(&mut tracker, failed, HEALTHY_WORLD_TIME * 2).is_empty());
}

#[test]
fn a_panic_is_forgotten_only_after_frames_keep_coming() {
    let mut tracker = RunTracker::default();
    let menu = frame(SessionPhase::Menus, false);
    tracker.observe(menu, FRAME);
    let panicked = Frame {
        panicked: true,
        ..menu
    };
    let survived = Changes {
        panic_survived: true,
        ..Changes::default()
    };
    // A frame or two can still run while an uncaught panic on another thread arrives.
    assert!(
        run_for(
            &mut tracker,
            panicked,
            PANIC_SURVIVAL_TIME - MAX_FRAME_CREDIT
        )
        .is_empty()
    );
    assert_eq!(tracker.observe(panicked, MAX_FRAME_CREDIT), survived);
    // Once forgotten the marker no longer reports it, and a later panic starts a new wait.
    assert!(run_for(&mut tracker, menu, PANIC_SURVIVAL_TIME * 2).is_empty());
    assert!(
        run_for(
            &mut tracker,
            panicked,
            PANIC_SURVIVAL_TIME - MAX_FRAME_CREDIT
        )
        .is_empty()
    );
    assert_eq!(
        tracker.observe(panicked, PANIC_SURVIVAL_TIME * 10),
        survived
    );
}

#[test]
fn serial_compilation_levels_leave_one_pipeline_worker_and_the_other_pools_alone() {
    use crate::thread_budget::ThreadBudget;
    for cores in [4, 8, 12] {
        let budget = ThreadBudget::for_cores(cores);
        for safe in SafeGraphics::ALL {
            let expected = ThreadBudget {
                async_compute: if safe.compiles_one_pipeline_at_a_time() {
                    1
                } else {
                    budget.async_compute
                },
                ..budget
            };
            assert_eq!(budget.for_safe_graphics(safe), expected, "{cores} {safe:?}");
        }
    }
    // The pool the engine compiles pipelines on gets exactly that one worker.
    let pools = ThreadBudget::task_pool_plugin(SafeGraphics::MAX).task_pool_options;
    assert_eq!(
        (
            pools.async_compute.min_threads,
            pools.async_compute.max_threads
        ),
        (1, 1)
    );
    assert!(SafeGraphics::MAX.compiles_one_pipeline_at_a_time());
    assert_eq!(ThreadBudget::for_cores(8).async_compute, 2);
}

#[test]
fn the_last_level_starts_in_vanilla_unless_the_launch_names_a_mode() {
    use crate::render_mode::{launch_override, startup_render_mode};
    use ui::RenderMode;
    for safe in SafeGraphics::ALL {
        let forced = (!safe.optional_effects()).then_some(RenderMode::Vanilla);
        assert_eq!(launch_override(None, safe), forced, "{safe:?}");
        for cli in RenderMode::ALL {
            assert_eq!(launch_override(Some(cli), safe), Some(cli), "{safe:?}");
        }
        // A saved shader-pack choice is drawn at every level but the last, and stays saved.
        let started = startup_render_mode(
            launch_override(None, safe),
            None,
            Some(RenderMode::ShaderPack),
            false,
        );
        let expected = if safe.optional_effects() {
            RenderMode::ShaderPack
        } else {
            RenderMode::Vanilla
        };
        assert_eq!(started, expected, "{safe:?}");
    }
    assert!(!SafeGraphics::MAX.optional_effects());
}

/// A device from the headless adapter, opened through the production request with the
/// settings `safe` configures.
fn device(safe: SafeGraphics) -> wgpu::Device {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions::enabled(),
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let mut settings = bevy::render::settings::WgpuSettings::default();
    render::device_settings::configure(&mut settings);
    render::device_settings::configure_safe_graphics(&mut settings, safe);
    bevy::tasks::block_on(super::super::device::request_device(&adapter, &settings))
        .unwrap()
        .0
}

#[test]
fn the_last_level_opens_a_device_without_indirect_terrain_draws_or_spare_storage() {
    let indirect =
        wgpu::Features::INDIRECT_FIRST_INSTANCE | wgpu::Features::MULTI_DRAW_INDIRECT_COUNT;
    let full = device(SafeGraphics::Off);
    assert!(full.features().contains(indirect));
    assert!(
        full.limits().max_storage_buffers_per_shader_stage
            >= render::required_vertex_storage_buffers()
    );
    for safe in SafeGraphics::ALL {
        let opened = device(safe);
        if safe.constrains_device() {
            assert!(!opened.features().intersects(indirect), "{safe:?}");
            assert_eq!(
                opened.limits().max_storage_buffers_per_shader_stage,
                render::minimum_vertex_storage_buffers(),
                "{safe:?}"
            );
            // Nothing else about the device is reduced.
            assert_eq!(opened.features() | indirect, full.features(), "{safe:?}");
            assert_eq!(
                wgpu::Limits {
                    max_storage_buffers_per_shader_stage: full
                        .limits()
                        .max_storage_buffers_per_shader_stage,
                    ..opened.limits()
                },
                full.limits(),
                "{safe:?}"
            );
        } else {
            assert_eq!(opened.features(), full.features(), "{safe:?}");
            assert_eq!(opened.limits(), full.limits(), "{safe:?}");
        }
    }
    assert!(SafeGraphics::MAX.constrains_device());
}
