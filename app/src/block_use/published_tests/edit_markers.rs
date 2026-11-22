//! Allow and deny blocks gate the cell a placement actually fills.
use super::*;

const USE_BUTTON: u8 = 2;
const EYE: [f32; 3] = [4.5, 2.620_01, 8.32];
/// A one-pickle cluster on the floor whose south side the fixture's ray reaches.
const PICKLE: [i32; 3] = [4, 1, 7];
/// The floor cell under the cell in front of the pickle's south face.
const FRONT_FLOOR: [i32; 3] = [4, 0, 8];
/// The floor cell under the pickle.
const PICKLE_FLOOR: [i32; 3] = [4, 0, 7];

/// The camera rotation whose ray reaches the pickle's south face below its top.
fn pickle_side_aim() -> Quat {
    Quat::from_rotation_x(-1.3476)
}

/// The floor fixture with a pickle at [`PICKLE`], `deny` turned into a deny block, and a
/// held sea pickle stack.
fn pickle_fixture(deny: [i32; 3]) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = floor_fixture(EYE, pickle_side_aim());
    set_blocks(
        &mut world,
        EYE,
        &[(PICKLE, "minecraft:sea_pickle"), (deny, "minecraft:deny")],
    );
    set_hotbar_stack(&mut world, 0, Some("minecraft:sea_pickle"));
    // The fixture's pick predates the pickle; take it again so the ray meets its side.
    let pick = frame_origin(&world, Vec3::from_array(EYE), pickle_side_aim());
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    retain_pick(&mut world, &pick);
    (world, captured)
}

/// The primary block the local client shows at `position`.
fn shown(world: &World, position: [i32; 3]) -> u32 {
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    sim::PaletteWorld::new(
        stream.collision_store(),
        world
            .resource::<PhysicsCollisionRegistries>()
            .registry(stream.network_id_mode()),
        stream.current_dimension(),
        Default::default(),
    )
    .primary_runtime_id(position)
    .unwrap()
}

/// The south face, whose neighbour lies in the next column along +z.
const SOUTH: u8 = 3;

/// Presses Use once on the fixture's pick and returns the clicked cells and faces sent.
fn press(world: &mut World, captured: &mut client_session::CapturedPackets) -> Vec<([i32; 3], u8)> {
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    hold(world, &mut router, Vec::new(), vec![USE_BUTTON]);
    world.run_system_cached(produce_block_use).unwrap();
    captured
        .drain()
        .iter()
        .filter_map(item_use)
        .map(|tx| (tx.position, tx.face))
        .collect()
}

/// A side-face stack fills the clicked pickle's cell, so a deny block under the cell in
/// front of that face does not refuse it.
#[test]
fn a_deny_block_beside_a_stacked_pickle_does_not_refuse_the_stack() {
    let (mut world, mut captured) = pickle_fixture(FRONT_FLOOR);
    let before = shown(&world, PICKLE);
    assert_eq!(press(&mut world, &mut captured), [(PICKLE, SOUTH)]);
    assert_ne!(
        shown(&world, PICKLE),
        before,
        "the stack is predicted in the pickle's own column"
    );
}

/// A deny block under the stacked pickle refuses the stack even when the cell in front of
/// the clicked face is unprotected.
#[test]
fn a_deny_block_under_a_stacked_pickle_refuses_the_stack() {
    let (mut world, mut captured) = pickle_fixture(PICKLE_FLOOR);
    let before = shown(&world, PICKLE);
    assert_eq!(press(&mut world, &mut captured), [(PICKLE, SOUTH)]);
    assert_eq!(
        shown(&world, PICKLE),
        before,
        "no stack is predicted over the deny block"
    );
}
