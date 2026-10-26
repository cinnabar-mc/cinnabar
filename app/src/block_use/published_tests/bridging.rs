//! Real input, physics, placement prediction and inventory in continuous bridge scenarios.
use super::frame_picks::{bridge_fixture, retain, successful_placement, warm_physics};
use super::*;

/// Simulates real forward movement over a ledge, local collision and packet production together.
#[test]
fn continuous_bridge_preserves_inventory_and_real_movement_modes() {
    for frame_micros in [6_250, 50_000, 100_000] {
        for pitch in [-0.8_f32, -0.9601, -1.1, -1.3, -1.5] {
            for (mode, keys, action) in [
                ("walk", vec![0x1a], None),
                ("jump", vec![0x1a, 0x2c], Some(Action::Jump)),
                ("sneak", vec![0x1a, 0xe1], Some(Action::Sneak)),
                ("sprint", vec![0x1a, 0xe0], Some(Action::Sprint)),
            ] {
                let eye = [4.5, 2.620_01, 6.1];
                let rotation = Quat::from_rotation_x(pitch);
                let (mut world, mut captured) = bridge_fixture(eye, rotation);
                warm_physics(&mut world);
                world
                    .resource_mut::<client_state::PlayerRuntime>()
                    .facts
                    .apply_hunger_attribute(&protocol::ActorAttribute {
                        name: protocol_ids::Attribute::PlayerHunger.wire_name().into(),
                        min: 0.0,
                        max: 20.0,
                        current: 20.0,
                        default: Some(20.0),
                        modifiers: [].into(),
                    });
                let mut router = crate::semantic_controls::SemanticInputRuntime::default();
                let mut placements = Vec::new();
                let mut attempts = 0;
                let initial_count = world
                    .resource::<client_state::PlayerRuntime>()
                    .selected_stack()
                    .unwrap()
                    .count;
                let mut ever_sprinting = false;
                let mut elapsed = 0;
                // Align the first press at the same game time for every presentation cadence.
                hold(&mut world, &mut router, keys.clone(), vec![2]);
                world.run_system_cached(produce_block_use).unwrap();
                let first = captured.drain();
                placements.extend(
                    first
                        .iter()
                        .filter_map(successful_placement)
                        .map(|placement| {
                            (
                                world.resource::<MovementTicker>().completed_tick() + 1,
                                gameplay::block_use::placement_cell(
                                    placement.position,
                                    placement.face,
                                ),
                            )
                        }),
                );
                while elapsed < 800_000 {
                    elapsed += frame_micros;
                    let snapshot = hold(&mut world, &mut router, keys.clone(), vec![2]);
                    if let Some(action) = action {
                        assert!(snapshot.phases[action as usize].held, "{mode} binding");
                    }
                    world
                        .resource_mut::<Time<Real>>()
                        .advance_by(Duration::from_micros(frame_micros));
                    world.run_system_cached(produce_block_use).unwrap();
                    let tick = world.resource::<MovementTicker>().completed_tick() + 1;
                    let packets = captured.drain();
                    attempts += packets.iter().filter_map(item_use).count();
                    for placement in packets.iter().filter_map(successful_placement) {
                        placements.push((
                            tick,
                            gameplay::block_use::placement_cell(placement.position, placement.face),
                        ));
                    }
                    world
                        .run_system_cached(crate::movement::advance_local_physics)
                        .unwrap();
                    let physics =
                        world.resource::<client_interaction::movement::LocalPhysicsController>();
                    ever_sprinting |= physics
                        .state()
                        .and_then(|state| physics.sample_at(state.tick))
                        .is_some_and(|sample| sample.processed.sprinting);
                    let pose = *world.resource::<LocalViewPose>();
                    retain(
                        &mut world,
                        pose.eye_translation().to_array(),
                        pose.rotation(),
                    );
                }
                let state = world
                    .resource::<MovementTicker>()
                    .build_action_state()
                    .unwrap();
                assert_eq!(
                    world
                        .resource::<client_state::PlayerRuntime>()
                        .selected_stack()
                        .unwrap()
                        .count,
                    initial_count - placements.len() as u16
                );
                assert_eq!(
                    ever_sprinting,
                    mode == "sprint",
                    "mode {mode}, cadence {frame_micros}, pitch {pitch}"
                );
                if mode == "sneak" {
                    assert!(state.position[1] > 2.6, "sneaking walked off the ledge");
                }
                println!(
                    "PLACEMENT_CONTINUOUS_BRIDGE frame_micros={frame_micros} pitch={pitch} mode={mode} attempts={attempts} placements={placements:?} final_position={:?}",
                    state.position
                );
            }
        }
    }
}
