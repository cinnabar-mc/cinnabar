//! A load always settles its request: a worker that fails or exits reports a visible failure
//! with Vanilla drawing, and a superseded worker never holds up the next pack.

use super::*;
use bevy::ecs::system::RunSystemOnce;

/// A world that runs [`drive_loader`] without a GPU, its load already running for `pack`.
fn running(pack: &str) -> (World, mpsc::Sender<LoadResult>) {
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
    progress.start();
    world.insert_resource(progress);
    let (send, receive) = mpsc::channel();
    world.insert_resource(Loader {
        key: Some((pack.into(), None, 0, 0)),
        job: Some(Mutex::new(receive)),
    });
    (world, send)
}

fn drive(world: &mut World) {
    world.run_system_once(drive_loader).unwrap();
}

fn failure(world: &World) -> Option<(PathBuf, String)> {
    match world.resource::<ShaderPackStatus>() {
        ShaderPackStatus::Failed { pack, reason } => Some((pack.clone(), reason.clone())),
        _ => None,
    }
}

#[test]
fn a_worker_that_exits_without_a_result_fails_its_load() {
    let (mut world, send) = running("Dusk.zip");
    drop(send);
    drive(&mut world);
    assert_eq!(
        failure(&world),
        Some(("Dusk.zip".into(), "the load worker stopped".to_owned()))
    );
}

#[test]
fn a_running_superseded_worker_does_not_hold_up_the_next_pack() {
    let (mut world, _running) = running("Dusk.zip");
    let mut gpu = super::super::plugin::native_tests::ready_world();
    world.insert_resource(gpu.remove_resource::<RenderDevice>().unwrap());
    world.insert_resource(gpu.remove_resource::<RenderQueue>().unwrap());
    world.insert_resource(gpu.remove_resource::<RenderAdapter>().unwrap());
    world.resource_mut::<ShaderPackRequest>().pack = Some("Lumen.zip".into());
    drive(&mut world);
    let loader = world.resource::<Loader>();
    assert!(loader.job.is_some(), "the next load starts at once");
    assert_eq!(loader.key.as_ref().unwrap().0, PathBuf::from("Lumen.zip"));
    // No compiler or pack exists, so the next load fails at its first stage and says so.
    test_time::eventually("the next pack's load settles", || {
        drive(&mut world);
        failure(&world).is_some()
    });
    let (pack, reason) = failure(&world).unwrap();
    assert_eq!(pack, PathBuf::from("Lumen.zip"));
    assert!(!reason.is_empty());
    assert!(world.resource::<Loader>().job.is_none());
}
