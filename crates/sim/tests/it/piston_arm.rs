use assets::block_shapes::{
    CompiledBlockShapes, OutlineContext, PickRule, SelectionContext, ShapeBox, ShapeFlags,
};
use sim::{
    Aabb, CollisionRegistry, CollisionWorld, MovementInput, MovementMode, PaletteWorld,
    PlayerState, Simulator, Vec3,
};
use world::{BlockEntityKey, BlockEntityNbt, BlockUpdate, ChunkKey, ChunkStore, NbtCompound};
use world::{NbtValue, SubChunkKey};

const AIR: u32 = 0;
const BODY: u32 = 1;
const UP: u8 = 1;
const EAST: u8 = 4;

/// Runtime ID of the arm state for one facing.
fn arm_id(facing: u8) -> u32 {
    10 + u32::from(facing)
}

/// Registers a full-cube piston body and upward and eastward arm states with no static
/// collision, as compiled for every piston arm, carrying their base, rod and head pieces.
fn registry() -> CollisionRegistry {
    let mut registry = CollisionRegistry::new();
    registry.register(AIR, []).unwrap();
    registry
        .register(BODY, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    for (facing, pieces) in [
        (
            UP,
            [
                ([0.3125, -0.25, 0.3125], [0.6875, 0.25, 0.6875]),
                ([0.375, 0.25, 0.375], [0.625, 0.75, 0.625]),
                ([0.0, 0.75, 0.0], [1.0; 3]),
            ],
        ),
        (
            EAST,
            [
                ([-0.25, 0.3125, 0.3125], [0.25, 0.6875, 0.6875]),
                ([0.25, 0.375, 0.375], [0.75, 0.625, 0.625]),
                ([0.75, 0.0, 0.0], [1.0; 3]),
            ],
        ),
    ] {
        let id = arm_id(facing);
        registry.register(id, []).unwrap();
        assert!(registry.set_compiled_selection(
            id,
            &CompiledBlockShapes {
                state_identity: [facing; 32],
                actor_facing: Some(facing),
                actor_visual: None,
                selection: pieces.map(|(min, max)| ShapeBox::new(min, max)).into(),
                selection_context: SelectionContext::PistonArm,
                contextual_selection: Box::new([]),
                outline: None,
                connected_outline: None,
                outline_context: OutlineContext::Static,
                pick_rule: PickRule::Always,
                flags: ShapeFlags::empty(),
                door: None,
                flow: None,
                tags: Box::new([]),
            }
        ));
    }
    registry
}

/// Two loaded columns either side of x = 16.
fn store() -> ChunkStore {
    let mut store = ChunkStore::new();
    for x in [0, 1] {
        store.mark_chunk_loaded(ChunkKey::new(0, x, 0)).unwrap();
    }
    store
}

/// Writes one primary block at non-negative coordinates.
fn set_block(store: &mut ChunkStore, [x, y, z]: [i32; 3], id: u32) {
    store
        .update_block(
            SubChunkKey::new(0, x >> 4, y >> 4, z >> 4),
            BlockUpdate::new((x & 15) as u8, (y & 15) as u8, (z & 15) as u8, 0, id),
            0,
        )
        .unwrap();
}

/// Places a piston body and its arm, with the body's actor at `progress` when present.
fn piston(store: &mut ChunkStore, body: [i32; 3], facing: u8, progress: Option<f32>) {
    let direction = assets::block_shapes::PISTON_DIRECTIONS[usize::from(facing)];
    set_block(store, body, BODY);
    set_block(
        store,
        std::array::from_fn(|axis| body[axis] + direction[axis]),
        arm_id(facing),
    );
    if let Some(progress) = progress {
        let mut root = NbtCompound::default();
        root.insert("id", NbtValue::String("PistonArm".into()));
        root.insert("Progress", NbtValue::Float(progress));
        let actor = BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
            .unwrap()
            .0;
        store
            .commit_block_entity_update(BlockEntityKey::new(0, body[0], body[1], body[2]), actor)
            .unwrap();
    }
}

/// Drops a player onto the column at (4.5, 4.5) and returns where its feet come to rest.
fn landing_height(store: &ChunkStore, registry: &CollisionRegistry) -> f64 {
    let world = PaletteWorld::for_movement(store, registry, 0);
    let mut state = PlayerState::new(Vec3::new(4.5, 8.0, 4.5));
    for _ in 0..80 {
        let tick = Simulator::default()
            .tick(&mut state, MovementInput::default(), &world)
            .unwrap();
        if tick.on_ground {
            return state.position.y;
        }
    }
    panic!("the player never landed");
}

#[test]
fn a_player_stands_on_the_head_of_an_upward_arm_at_its_progress() {
    let registry = registry();
    for (progress, expected) in [(None, 5.0), (Some(1.0), 6.0), (Some(0.5), 5.5)] {
        let mut store = store();
        piston(&mut store, [4, 4, 4], UP, progress);
        let feet = landing_height(&store, &registry);
        assert!(
            (feet - expected).abs() < 1e-6,
            "progress {progress:?}: landed at {feet}, expected {expected}"
        );
    }
}

#[test]
fn walking_into_a_sideways_arm_stops_at_its_head() {
    let registry = registry();
    let mut store = store();
    piston(&mut store, [4, 4, 4], EAST, Some(1.0));
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let mut state = PlayerState::new(Vec3::new(7.0, 4.0, 4.5));
    let mut blocked = false;
    for _ in 0..10 {
        state.velocity = Vec3::new(-0.5, 0.0, 0.0);
        let input = MovementInput {
            mode: MovementMode::Flying,
            ..Default::default()
        };
        blocked |= Simulator::default()
            .tick(&mut state, input, &world)
            .unwrap()
            .collisions
            .x;
    }
    assert!(blocked);
    assert!(
        (Aabb::player_at(state.position).min.x - 6.0).abs() < 1e-6,
        "player stopped at x = {}",
        state.position.x
    );
}

#[test]
fn arm_collision_identity_includes_the_owner_column_outside_the_scan() {
    let registry = registry();
    let mut store = store();
    // The body sits at x = 15 and the eastward arm at x = 16, across the column seam.
    piston(&mut store, [15, 4, 4], EAST, Some(1.0));
    let query = Aabb::new(Vec3::new(17.0, 4.0, 4.0), Vec3::new(17.5, 5.0, 5.0));
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let boxes = world.collision_boxes_with_provenance(query).unwrap();
    let owner = ChunkKey::new(0, 0, 0);
    assert!(
        boxes
            .identity
            .chunks
            .iter()
            .any(|revision| revision.chunk == owner)
    );
    piston(&mut store, [15, 4, 4], EAST, Some(0.5));
    let moved = PaletteWorld::new(&store, &registry, 0, Default::default())
        .collision_boxes_with_provenance(query)
        .unwrap();
    assert_ne!(moved.identity, boxes.identity);
}
