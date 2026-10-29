//! A load always ends: a worker that fails, stops or stops making progress settles its request
//! as a visible failure with Vanilla drawing, and never holds up the next pack.

use super::*;
use bevy::ecs::system::RunSystemOnce;

/// A world that runs [`drive_loader`] without a GPU, its load already running for `pack`.
fn running(pack: &str) -> (World, Arc<LoadProgress>, mpsc::Sender<LoadResult>) {
    let mut world = World::new();
    world.insert_resource(ShaderPackRequest {
        pack: Some(pack.into()),
        ..default()
    });
    world.init_resource::<ShaderPackFrameInputs>();
    world.init_resource::<ShaderPackSettledRevision>();
    world.init_resource::<LoadedShaderPack>();
    world.insert_resource(ShaderPackStatus::Loading(pack.into()));
    let mut progress = ShaderPackLoadProgress::default();
    let worker = progress.start();
    world.insert_resource(progress);
    let (send, receive) = mpsc::channel();
    world.insert_resource(Loader {
        key: Some((pack.into(), None, 0, 0)),
        job: Some(Job::new(receive, Arc::clone(&worker))),
        ..default()
    });
    let mut time = Time::<Real>::default();
    time.update_with_duration(Duration::ZERO);
    world.insert_resource(time);
    (world, worker, send)
}

/// Runs the driver once, `by` after the previous run.
fn drive(world: &mut World, by: Duration) {
    world.resource_mut::<Time<Real>>().update_with_duration(by);
    world.run_system_once(drive_loader).unwrap();
}

fn failure(world: &World) -> Option<(PathBuf, String)> {
    match world.resource::<ShaderPackStatus>() {
        ShaderPackStatus::Failed { pack, reason } => Some((pack.clone(), reason.clone())),
        _ => None,
    }
}

#[test]
fn a_worker_that_stops_making_progress_fails_its_load() {
    let (mut world, worker, _held) = running("Dusk.zip");
    worker.begin("building gbuffers_entities for Entities");
    drive(&mut world, Duration::ZERO);
    drive(&mut world, STALL_LIMIT - Duration::from_secs(1));
    assert_eq!(failure(&world), None, "a slow step is not a stall");
    worker.record_built(1, 3, "building final");
    drive(&mut world, Duration::from_secs(1));
    drive(&mut world, STALL_LIMIT - Duration::from_secs(1));
    assert_eq!(failure(&world), None, "progress restarts the limit");
    drive(&mut world, Duration::from_secs(1));
    let (pack, reason) = failure(&world).expect("the stalled load fails");
    assert_eq!(pack, PathBuf::from("Dusk.zip"));
    assert!(
        reason.starts_with("stopped responding while building final"),
        "{reason}"
    );
    assert!(worker.is_cancelled(), "the stalled worker is told to stop");
    assert!(world.resource::<Loader>().job.is_none());
    assert!(world.resource::<LoadedShaderPack>().0.is_none());
    assert!(!world.resource::<ShaderPackLoadProgress>().is_loading());
}

#[test]
fn a_worker_that_exits_without_a_result_fails_its_load() {
    let (mut world, _worker, send) = running("Dusk.zip");
    drop(send);
    drive(&mut world, Duration::ZERO);
    assert_eq!(
        failure(&world),
        Some(("Dusk.zip".into(), "the load worker stopped".to_owned()))
    );
}

#[test]
fn a_stalled_superseded_worker_does_not_hold_up_the_next_pack() {
    let (mut world, _worker, _held) = running("Dusk.zip");
    let mut gpu = super::super::plugin::native_tests::ready_world();
    world.insert_resource(gpu.remove_resource::<RenderDevice>().unwrap());
    world.insert_resource(gpu.remove_resource::<RenderQueue>().unwrap());
    world.insert_resource(gpu.remove_resource::<RenderAdapter>().unwrap());
    world.resource_mut::<ShaderPackRequest>().pack = Some("Lumen.zip".into());
    drive(&mut world, Duration::ZERO);
    assert!(
        world.resource::<Loader>().retiring,
        "waits for the old worker"
    );
    assert_eq!(
        *world.resource::<ShaderPackStatus>(),
        ShaderPackStatus::Loading("Lumen.zip".into())
    );
    drive(&mut world, STALL_LIMIT);
    let loader = world.resource::<Loader>();
    assert!(!loader.retiring && loader.job.is_some());
    assert_eq!(loader.key.as_ref().unwrap().0, PathBuf::from("Lumen.zip"));
    // No compiler or pack exists, so the next load fails at its first stage and says so.
    test_time::eventually("the next pack's load settles", || {
        drive(&mut world, Duration::ZERO);
        failure(&world).is_some()
    });
    let (pack, reason) = failure(&world).unwrap();
    assert_eq!(pack, PathBuf::from("Lumen.zip"));
    assert!(!reason.is_empty());
    assert!(world.resource::<Loader>().job.is_none());
}
