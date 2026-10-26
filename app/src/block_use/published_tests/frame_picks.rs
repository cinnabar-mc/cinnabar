//! Held placement consumes every presented pick and refreshes its original geometry.
use super::*;

/// Completes the post-reanchor frame without consuming a movement tick.
pub(super) fn warm_physics(world: &mut World) {
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::ZERO);
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
}

/// Retains a published ray belonging to the fixture's current movement authority.
pub(super) fn retain(world: &mut World, eye: [f32; 3], rotation: Quat) {
    let origin = frame_origin(world, Vec3::from_array(eye), rotation);
    retain_pick(world, &origin);
    world.insert_resource(origin);
}

/// Removes the shared fixture's chest and publishes the requested initial view for bridge use.
pub(super) fn bridge_fixture(
    eye: [f32; 3],
    rotation: Quat,
) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = floor_fixture(eye, rotation);
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let air = stream.air_block_id();
        stream
            .submit(
                4,
                protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                    dimension: 0,
                    position: [4, 2, 6],
                    layer: 0,
                    network_id: air,
                }]),
            )
            .unwrap();
        test_time::eventually("placement probe clears fixture chest", || {
            stream.poll(eye, 0);
            stream.committed_sequence() >= 4
        });
    }
    world.insert_resource(LocalViewPose::new(Vec3::from_array(eye), rotation));
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    retain(&mut world, eye, rotation);
    (world, captured)
}

/// Recognizes a successful survival placement by its inventory delta, excluding block interactions.
pub(super) fn successful_placement(
    packet: &protocol::Packet,
) -> Option<protocol::test_support::transactions::ItemUseObservation> {
    let transaction = protocol::test_support::transactions::observe_transaction(packet)?;
    let use_on = transaction.item_use()?;
    (use_on.predicted_success && !transaction.actions.is_empty()).then(|| use_on.clone())
}

/// Replays all binary hit/miss sequences with one to four frame picks before a tick.
#[test]
fn all_frame_picks_reach_the_build_action() {
    let eye = [4.5, 2.620_01, 8.32];
    let good = Quat::from_rotation_x(-0.9601);
    let mut expected_successes = 0;
    let mut actual_successes = 0;
    let mut cases = Vec::new();
    for frames in 1..=4 {
        for mask in 0..(1 << frames) {
            let (mut world, mut captured) = bridge_fixture(eye, good);
            warm_physics(&mut world);
            hold_use(&mut world);
            world.resource_mut::<BlockUseRuntime>().clear_picks();
            for frame in 0..frames {
                retain(
                    &mut world,
                    eye,
                    if mask & (1 << frame) != 0 {
                        good
                    } else {
                        Quat::IDENTITY
                    },
                );
            }
            world
                .resource_mut::<Time<Real>>()
                .advance_by(Duration::from_millis(50));
            world.run_system_cached(produce_block_use).unwrap();
            let packets = captured.drain();
            let placements: Vec<_> = packets.iter().filter_map(successful_placement).collect();
            let successes = placements.len();
            for placement in placements {
                assert_eq!(placement.position, [4, 0, 7]);
                assert_eq!(placement.face, 1);
            }
            // Every nonzero mask contains an unobstructed placement with an elapsed deadline.
            expected_successes += usize::from(mask != 0);
            actual_successes += successes;
            cases.push((frames, mask, successes));
            assert!(successes <= 1);
        }
    }
    assert_eq!(
        (expected_successes, actual_successes),
        (26, 26),
        "{cases:?}"
    );
}

/// Observes the production target without committing packets or modifying the placement line.
fn target(world: &mut World) -> Option<FrozenBlockObservation> {
    target_for_mode(world, PlayerInputMode::Mouse, true)
}

/// Resolves the retained observation with the current input-mode reach.
fn target_for_mode(
    world: &mut World,
    input_mode: PlayerInputMode,
    survival: bool,
) -> Option<FrozenBlockObservation> {
    let mut system = bevy::ecs::system::SystemState::<(
        BlockUseContext,
        Res<client_state::PlayerRuntime>,
        Res<BlockUseRuntime>,
        Res<MovementTicker>,
    )>::new(world);
    let (context, player, runtime, movement) = system.get_mut(world).unwrap();
    let input = context.input.snapshot()?;
    observe_use_target(
        &player,
        &context,
        &context.origin,
        input_mode,
        survival,
        (input.authority_generation, input.frame_sequence),
        movement.position_authority_identity().1,
        &runtime,
        runtime.pick(&movement),
        ItemUseTrigger::SimulationTick,
        &movement.build_action_state()?,
    )
}

/// Demonstrates an added support-centre rejection after the placement-line segment passes.
#[test]
fn a_locked_line_uses_its_bounded_segment() {
    let eye = [4.5, 2.620_01, 12.1];
    let rotation = Quat::from_rotation_x(-0.42);
    let (mut world, _) = floor_fixture(eye, rotation);
    let air = {
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        client.stream.as_ref().unwrap().air_block_id()
    };
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let updates = (3..=5)
            .flat_map(|x| (6..=9).map(move |z| [x, 0, z]))
            .filter(|position| *position != [4, 0, 6])
            .map(|position| protocol::BlockUpdateEvent {
                dimension: 0,
                position,
                layer: 0,
                network_id: air,
            })
            .collect();
        stream
            .submit(4, protocol::WorldEvent::BlockUpdates(updates))
            .unwrap();
        test_time::eventually("placement probe isolated support", || {
            stream.poll(eye, 0);
            stream.committed_sequence() >= 4
        });
    }
    retain(&mut world, eye, rotation);
    {
        let mut runtime = world.resource_mut::<BlockUseRuntime>();
        for (repeated, destination) in [(false, [4, 0, 5]), (true, [4, 0, 6])] {
            runtime.intention.record(
                repeated,
                destination,
                LocalUse::Place,
                true,
                false,
                [4.5, 1.0, 6.5],
            );
        }
    }
    let actual = target(&mut world);
    assert_eq!(
        actual.map(|hit| (hit.target.position, hit.target.face)),
        Some(([4, 0, 6], 3))
    );
}

/// Applies changed world geometry without publishing another frame pick.
pub(super) fn update(world: &mut World, position: [i32; 3], name: &str) {
    let block = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .find(|block| block.name.as_ref() == name)
    .unwrap()
    .sequential_id;
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                dimension: 0,
                position,
                layer: 0,
                network_id: block,
            }]),
        )
        .unwrap();
    test_time::eventually("changed pick geometry", || {
        stream.poll([4.5, 2.620_01, 8.5], 0);
        stream.committed_sequence() >= sequence
    });
}

/// Removing the originally picked support must not extend the old ray to a farther block.
#[test]
fn refresh_keeps_the_original_intercept_when_its_block_disappears() {
    let (mut world, _) = fixture();
    let original = world
        .resource::<BlockUseRuntime>()
        .pick(world.resource::<MovementTicker>())
        .unwrap()
        .hits
        .ordinary
        .unwrap()
        .point()
        .unwrap();
    update(&mut world, [4, 2, 6], "minecraft:air");
    update(&mut world, [4, 2, 4], "minecraft:stone");
    let hit = target(&mut world).expect("original block fallback");
    assert_eq!(hit.target.position, [4, 2, 6]);
    assert_eq!(hit.target.hit_world, original);
}

/// A new blocker inside the original segment replaces the saved support.
#[test]
fn refresh_selects_a_new_nearer_occluder() {
    let (mut world, _) = fixture();
    update(&mut world, [4, 2, 7], "minecraft:stone");
    assert_eq!(target(&mut world).unwrap().target.position, [4, 2, 7]);
}

/// A blocked placement does not consume the later valid pick in the same simulation tick.
#[test]
fn a_failed_placement_retries_the_next_buffered_pick() {
    let eye = [4.5, 2.620_01, 8.32];
    let good = Quat::from_rotation_x(-0.9601);
    let (mut world, mut captured) = bridge_fixture(eye, good);
    warm_physics(&mut world);
    hold_use(&mut world);
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    retain(
        &mut world,
        eye,
        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
    );
    retain(&mut world, eye, good);
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(50));
    world.run_system_cached(produce_block_use).unwrap();
    let uses = captured
        .drain()
        .iter()
        .filter_map(item_use)
        .collect::<Vec<_>>();
    assert_eq!(
        uses.iter()
            .map(|tx| (tx.position, tx.predicted_success))
            .collect::<Vec<_>>(),
        [([4, 0, 8], false), ([4, 0, 7], true)]
    );
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured.drain().is_empty(),
        "consumed frame picks must not be retried without another publication"
    );
}

/// An unsupported hand use consumes only its own observation, allowing a later chest interaction.
#[test]
fn an_unusable_first_pick_does_not_discard_a_later_block_interaction() {
    let (mut world, mut captured) = fixture();
    update(&mut world, [3, 2, 6], "minecraft:stone");
    let state = world
        .resource::<MovementTicker>()
        .build_action_state()
        .unwrap();
    let mut physics = client_interaction::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(state.position, state.tick, true);
    world.insert_resource(physics);
    hold_use(&mut world);
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    let eye = [4.5, 2.5, 8.5];
    retain(
        &mut world,
        eye,
        Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::new(-1.0, 0.0, -2.0).normalize()),
    );
    retain(&mut world, eye, Quat::IDENTITY);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
}

/// A picked actor still bounds the ray segment used to continue an already established line.
#[test]
fn a_placement_line_can_continue_in_front_of_a_picked_actor() {
    let eye = [4.5, 2.620_01, 8.32];
    let rotation = Quat::from_rotation_x(-0.42);
    let (mut world, mut captured) = bridge_fixture(eye, rotation);
    update(&mut world, [4, 1, 7], "minecraft:stone");
    actor_use_tests::spawn(&mut world, 5.4);
    warm_physics(&mut world);
    hold_use(&mut world);
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    retain(&mut world, eye, rotation);
    {
        let mut runtime = world.resource_mut::<BlockUseRuntime>();
        for (repeated, destination) in [(false, [4, 1, 8]), (true, [4, 1, 7])] {
            runtime.intention.record(
                repeated,
                destination,
                LocalUse::Place,
                true,
                false,
                [4.5, 2.0, 7.5],
            );
        }
    }
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(50));
    world.run_system_cached(produce_block_use).unwrap();
    let placements = captured
        .drain()
        .iter()
        .filter_map(successful_placement)
        .collect::<Vec<_>>();
    assert_eq!(placements.len(), 1);
    assert_eq!(
        gameplay::block_use::placement_cell(placements[0].position, placements[0].face),
        [4, 1, 6]
    );
}

/// A later longer reach cannot extend the geometry represented by an earlier miss.
#[test]
fn a_saved_miss_keeps_its_segment_when_the_current_reach_grows() {
    let (mut world, _) = bridge_fixture([4.5, 2.620_01, 11.5], Quat::IDENTITY);
    let pick = world
        .resource::<BlockUseRuntime>()
        .pick(world.resource::<MovementTicker>())
        .unwrap();
    assert_eq!(
        pick.hits.ordinary,
        Some(gameplay::block_use::PickResult::Miss)
    );
    update(&mut world, [4, 2, 4], "minecraft:stone");
    {
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_ref().unwrap();
        let collisions = world.resource::<PhysicsCollisionRegistries>();
        let palette = PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            0,
            Default::default(),
        );
        let extended = palette
            .block_interaction_ray_current(
                sim::Vec3::new(4.5, 2.620_01_f32.into(), 11.5),
                sim::Vec3::new(0.0, 0.0, -1.0),
                gameplay::mining::creative_reach(PlayerInputMode::Touch),
            )
            .expect("extended ray stays in loaded terrain")
            .expect("extended ray reaches the added block");
        assert_eq!(extended.block_pos, [4, 2, 4]);
    }
    assert!(target_for_mode(&mut world, PlayerInputMode::Touch, false).is_none());
}

/// A server overlay can be the only selectable geometry in its host cell.
#[test]
fn buffered_picks_and_refresh_keep_server_overlay_geometry() {
    use world::{NbtCompound, NbtValue};
    let (mut world, _) = fixture();
    let state = |name: &str| {
        let mut state = NbtCompound::default();
        state.insert("name", NbtValue::String(name.into()));
        state.insert("states", NbtValue::Compound(NbtCompound::default()));
        NbtValue::Compound(state)
    };
    let mut overlay = NbtCompound::default();
    overlay.insert("slot", NbtValue::Byte(3));
    overlay.insert("face", NbtValue::Byte(2));
    overlay.insert("block", state("minecraft:stone"));
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("CinnabarOverlays".into()));
    root.insert("host", state("minecraft:air"));
    root.insert(
        "overlays",
        NbtValue::List(vec![NbtValue::Compound(overlay)]),
    );
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let sequence = stream.committed_sequence();
        stream
            .submit(
                sequence + 1,
                protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                    dimension: 0,
                    position: [4, 2, 6],
                    layer: 0,
                    network_id: stream.air_block_id(),
                }]),
            )
            .unwrap();
        stream
            .submit(
                sequence + 2,
                protocol::WorldEvent::BlockEntityUpdate(protocol::BlockEntityUpdateEvent {
                    dimension: 0,
                    position: [4, 2, 6],
                    nbt: root.encode_root().unwrap(),
                }),
            )
            .unwrap();
        test_time::eventually("server overlay commit", || {
            stream.poll([4.5, 2.5, 8.5], 0);
            stream.committed_sequence() >= sequence + 2
        });
    }
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    retain(&mut world, [4.5, 2.5, 8.5], Quat::IDENTITY);
    let pick = world
        .resource::<BlockUseRuntime>()
        .pick(world.resource::<MovementTicker>())
        .unwrap();
    let Some(gameplay::block_use::PickResult::Block(block)) = pick.hits.ordinary else {
        panic!("the overlay supplies a block hit even though its host is air");
    };
    assert_eq!(block.position, [4, 2, 6]);
    assert_eq!(block.point, [4.5, 2.5, 7.0]);
    assert_eq!(block.overlay.unwrap().slot, 3);
    let refreshed = target(&mut world).unwrap();
    assert_eq!(refreshed.target.position, [4, 2, 6]);
    assert_eq!(refreshed.target.hit_world, block.point);
}
