//! Block selection and mining share the frame's final centre-distance trim.

use super::*;

/// Moves the fixture obstruction and aims its eye-origin ray at the block centre.
fn target(world: &mut World, block: [i32; 3], eye: [f32; 3], position: [f32; 3]) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let runtime_id = stream
        .collision_store()
        .sub_chunk_ref(world::SubChunkKey::new(0, 0, 0, 0))
        .unwrap()
        .runtime_id(0, 4, 2, 6)
        .unwrap();
    stream.predict_block([4, 2, 6], 0, stream.air_block_id());
    stream.predict_block(block, 0, runtime_id);
    let centre = Vec3::from_array(block.map(|axis| axis as f32 + 0.5));
    let eye = Vec3::from_array(eye);
    let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, (centre - eye).normalize());
    let origin = frame_origin(world, eye, rotation);
    world.insert_resource(origin);
    let mut physics = client_interaction::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(position, 101, true);
    world.insert_resource(physics);
}

#[test]
fn block_pick_reach_diagonal_and_upward_edges_trim_outline_and_mining_together() {
    for (block, eye, expected) in [
        ([8, 2, 4], [4.1, 2.5, 8.9], false),
        ([8, 7, 8], [4.9, 2.5, 8.9], false),
        ([8, 2, 4], [4.9, 2.5, 8.1], true),
        ([8, 7, 8], [5.5, 3.0, 8.5], true),
    ] {
        let (mut world, _) = super::attack_hit_position_tests::unpublished_attack_fixture(-30.0);
        target(&mut world, block, eye, eye);
        world.init_resource::<client_state::RuntimeSettings>();
        world.init_resource::<render_scene::BlockSelectionFrameResource>();
        world.init_resource::<crate::block_selection::CrosshairActor>();
        world
            .run_system_cached(crate::block_selection::publish)
            .unwrap();
        assert_eq!(
            !world
                .resource::<render_scene::BlockSelectionFrameResource>()
                .outline
                .is_empty(),
            expected,
            "outline block={block:?} eye={eye:?}"
        );
        super::attack_hit_position_tests::attack(&mut world);
        assert_eq!(
            world
                .resource::<crate::melee::MeleeTarget>()
                .attack
                .unwrap()
                .crosshair,
            if expected {
                gameplay::melee::Crosshair::Block
            } else {
                gameplay::melee::Crosshair::Miss
            },
            "mining block={block:?} eye={eye:?}"
        );
    }
}

#[test]
fn block_pick_reach_trims_from_ticked_position_instead_of_the_interpolated_eye() {
    let (mut world, _) = super::attack_hit_position_tests::unpublished_attack_fixture(-30.0);
    target(&mut world, [4, 2, 3], [4.5, 2.5, 8.5], [4.5, 2.5, 9.5]);
    world.init_resource::<client_state::RuntimeSettings>();
    world.init_resource::<render_scene::BlockSelectionFrameResource>();
    world.init_resource::<crate::block_selection::CrosshairActor>();
    world
        .run_system_cached(crate::block_selection::publish)
        .unwrap();
    assert!(
        world
            .resource::<render_scene::BlockSelectionFrameResource>()
            .outline
            .is_empty()
    );
    super::attack_hit_position_tests::attack(&mut world);
    assert_eq!(
        world
            .resource::<crate::melee::MeleeTarget>()
            .attack
            .unwrap()
            .crosshair,
        gameplay::melee::Crosshair::Miss
    );
}
