//! Held block-use attempts on crops whose harvest is handled by the server.
use super::*;

const EYE: [f32; 3] = [4.5, 2.620_01, 8.5];
const CROP: [i32; 3] = [4, 1, 6];
const PITCH: f32 = -0.65;

/// Supplies the requested wheat growth on farmland, aimed inside its selection shape.
fn wheat_fixture(growth: u64) -> (World, client_session::CapturedPackets) {
    let rotation = Quat::from_rotation_x(PITCH);
    let (mut world, captured) = frame_picks::bridge_fixture(EYE, rotation);
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let wheat = records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:wheat"
                && serde_json::from_str::<serde_json::Value>(&record.canonical_state).unwrap()
                    [assets::facts::BlockProperty::Growth.name()]["value"]
                    .as_u64()
                    == Some(growth)
        })
        .unwrap()
        .sequential_id;
    let farmland = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:farmland")
        .unwrap()
        .sequential_id;
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let sequence = stream.committed_sequence() + 1;
        stream
            .submit(
                sequence,
                protocol::WorldEvent::BlockUpdates(
                    [(CROP, wheat), ([CROP[0], CROP[1] - 1, CROP[2]], farmland)]
                        .into_iter()
                        .map(|(position, network_id)| protocol::BlockUpdateEvent {
                            dimension: 0,
                            position,
                            layer: 0,
                            network_id,
                        })
                        .collect(),
                ),
            )
            .unwrap();
        test_time::eventually("harvest fixture terrain", || {
            stream.poll(EYE, 0);
            stream.committed_sequence() >= sequence
        });
    }
    (world, captured)
}

/// Failed local crop uses still send due attempts without predicting a server harvest.
#[test]
fn held_wheat_use_retries_without_local_success() {
    for item in [
        None,
        Some("minecraft:stone_hoe"),
        Some("minecraft:diamond_hoe"),
        Some("minecraft:diamond_pickaxe"),
    ] {
        check_held_wheat_use(item, 7, true);
    }
}

/// Item-specific use outcomes cannot borrow the failed-use repeat clock.
#[test]
fn unpredicted_item_specific_crop_uses_do_not_repeat() {
    for item in ["minecraft:bone_meal", "minecraft:chicken_spawn_egg"] {
        check_held_wheat_use(Some(item), 0, false);
    }
}

/// Drives real physics and both use owners while checking the expected crop attempts.
fn check_held_wheat_use(item: Option<&str>, growth: u64, repeats: bool) {
    let (mut world, mut captured) = wheat_fixture(growth);
    if let Some(identifier) = item {
        item_use_tests::hold_air_item(&mut world, identifier, 1, [].into());
    } else {
        set_hotbar_stack(&mut world, 0, None);
    }
    world.init_resource::<client_presentation::aim_assist::AimAssistFrame>();
    world.init_resource::<client_presentation::camera::ServerCameraView>();
    let before = world
        .resource::<client_state::PlayerRuntime>()
        .selected_stack()
        .cloned();
    let rotation = Quat::from_rotation_x(PITCH);
    frame_picks::warm_physics(&mut world);
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    frame_picks::retain(&mut world, EYE, rotation);
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    for frame in 0..=20 {
        hold(&mut world, &mut router, vec![], vec![2]);
        let pick = world
            .resource::<BlockUseRuntime>()
            .pick(world.resource::<MovementTicker>())
            .unwrap();
        assert!(
            matches!(pick.hits.ordinary, Some(gameplay::block_use::PickResult::Block(hit)) if hit.position == CROP),
            "{item:?}, frame {frame} must target the crop"
        );
        if frame > 0 {
            world
                .resource_mut::<Time<Real>>()
                .advance_by(Duration::from_millis(50));
        }
        world.run_system_cached(produce_block_use).unwrap();
        let packets = captured.drain();
        let uses = packets.iter().filter_map(item_use).collect::<Vec<_>>();
        // The press leaves its ray buffered. The first tick can retry that ray and
        // the newly presented one; later ticks receive one new ray each.
        let expected = if frame == 0 {
            1
        } else if repeats {
            1 + usize::from(frame == 1)
        } else {
            0
        };
        assert_eq!(uses.len(), expected, "{item:?}, frame {frame}");
        for use_on in &uses {
            assert_eq!(use_on.position, CROP);
            assert_eq!(
                use_on.trigger,
                Some(if frame == 0 {
                    ItemUseTrigger::PlayerInput
                } else {
                    ItemUseTrigger::SimulationTick
                })
            );
            assert!(!use_on.predicted_success);
            assert!(!use_on.cooldown_active);
        }
        assert_eq!(
            packets.len(),
            uses.len(),
            "failed uses must not swing or start use"
        );
        world
            .run_system_cached(crate::movement::advance_local_physics)
            .unwrap();
        let pose = *world.resource::<LocalViewPose>();
        frame_picks::retain(&mut world, pose.eye_translation().to_array(), rotation);
        produce_air_use(&mut world);
        captured.drain();
    }
    assert!(world.resource::<MovementTicker>().completed_tick() >= 120);
    assert_eq!(
        world
            .resource::<client_state::PlayerRuntime>()
            .selected_stack(),
        before.as_ref(),
        "failed uses must not consume the held stack"
    );
}
