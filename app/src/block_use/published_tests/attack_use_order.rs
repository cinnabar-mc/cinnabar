//! Attack taps decide whether Use in the same frame may continue.

use super::*;

/// Drives a tapped attack plus a Use press in one frame; melee resolves the attack against
/// `crosshair` before the next frame. Returns the block transactions sent over both frames.
fn attack_tap_then_use(crosshair: gameplay::melee::Crosshair) -> Vec<[i32; 3]> {
    let (mut world, mut captured) = fixture();
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    let frame = |router: &mut crate::semantic_controls::SemanticInputRuntime, tap: bool| {
        router
            .route_and_finalize(semantic_input::DeviceFrame {
                keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                    mouse_buttons: vec![2],
                    mouse_edges: semantic_input::ButtonEdges {
                        pressed: if tap { vec![1, 2] } else { Vec::new() },
                        released: if tap { vec![1] } else { Vec::new() },
                    },
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap()
    };
    let tapped = frame(&mut router, true);
    let attack = tapped.phases[Action::Attack as usize];
    assert!(
        attack.pressed && !attack.held,
        "the attack was tapped between frames"
    );
    assert!(tapped.phases[Action::Use as usize].pressed);
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(tapped),
    ));
    world.run_system_cached(produce_block_use).unwrap();
    // Melee runs later in the frame and resolves the same tap.
    let mut melee = world.resource_mut::<client_interaction::melee::MeleeRuntime>();
    assert!(melee.observe_input(true, false));
    melee.resolve(
        crosshair,
        &gameplay::melee::PressContext {
            refused_destroy: None,
            actor_life: None,
            tick: 102,
            player_position: [4.5, 2.620_01, 8.5],
            input_mode: protocol::PlayerInputMode::Mouse,
            local_runtime_id: 42,
            selection: None,
            swing_duration: 6,
            item_attack: None,
            now_millis: 1_000,
        },
        &mut gameplay::melee::SwingTracker::default(),
    );
    let held = frame(&mut router, false);
    assert!(!held.phases[Action::Use as usize].pressed);
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(held),
    ));
    world.run_system_cached(produce_block_use).unwrap();
    transaction_targets(&mut captured)
}

/// An attack handled first holds off a Use pressed in the same frame: only the hit lands.
#[test]
fn a_tapped_actor_attack_holds_off_the_same_frame_use() {
    let hit = gameplay::melee::Crosshair::Actor(gameplay::melee::ActorHit {
        runtime_id: 9,
        distance: 2.0,
        point: [4.5, 2.5, 6.5],
    });
    assert!(attack_tap_then_use(hit).is_empty());
}

/// A tap that only swings at a block leaves the Use to resolve on the next frame.
#[test]
fn a_tapped_block_attack_lets_the_same_frame_use_follow() {
    assert_eq!(
        attack_tap_then_use(gameplay::melee::Crosshair::Block),
        [[4, 2, 6]]
    );
}
