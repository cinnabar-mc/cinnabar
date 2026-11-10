//! Admission behavior is exercised through physical input and captured packets in block-use tests.

use super::*;

#[test]
fn maintenance_reads_current_buttons_after_neutral_rearm() {
    let mut router = semantic_input::SemanticInputRouter::default();
    router
        .route(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_buttons: vec![1],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let input = SemanticInputSnapshot(semantic_input::finalized::FinalizedInput::from_finalized(
        router.finalize().unwrap(),
    ));
    let mut runtime = InteractionRuntime::default();
    runtime.frames.quarantine();
    assert!(!held(Some(&runtime), &input, Action::Attack));
    runtime.frames.observe_neutral(true);
    assert!(held(Some(&runtime), &input, Action::Attack));
    assert!(!is_blocked(Some(&runtime)));
}

/// Counts its runs in its own state and adds the input, so shared registrations show.
fn counted(In(step): In<u32>, mut runs: Local<u32>) -> u32 {
    *runs += step;
    *runs
}

#[test]
fn cached_runs_share_the_registration_of_run_system_cached() {
    let mut world = World::new();
    assert_eq!(run_cached(&mut world, counted, 1).unwrap(), 1);
    assert_eq!(world.run_system_cached_with(counted, 2).unwrap(), 3);
    assert_eq!(run_cached(&mut world, counted, 3).unwrap(), 6);
    // A removed registration is rebuilt exactly as `run_system_cached_with` rebuilds it.
    world.unregister_system_cached(counted).unwrap();
    assert_eq!(run_cached(&mut world, counted, 4).unwrap(), 4);
    assert_eq!(world.run_system_cached_with(counted, 1).unwrap(), 5);
}
