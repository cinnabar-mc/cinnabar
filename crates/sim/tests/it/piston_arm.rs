use assets::block_shapes::{
    CompiledBlockShapes, OutlineContext, PickRule, SelectionContext, ShapeBox, ShapeFlags,
};
use sim::{
    Aabb, CollisionRegistry, CollisionWorld, MovementInput, MovementMode, PaletteWorld,
    PlayerState, PredictionHistory, Simulator, Vec3,
};
use world::{BlockEntityKey, BlockEntityNbt, BlockUpdate, ChunkKey, ChunkStore, NbtCompound};
use world::{NbtValue, SubChunkKey};

const AIR: u32 = 0;
const BODY: u32 = 1;
const UP: u8 = 1;
const EAST: u8 = 4;
/// Piston actor `State` bytes.
const RETRACTED: i8 = 0;
const EXPANDING: i8 = 1;
const EXPANDED: i8 = 2;
const RETRACTING: i8 = 3;

/// Runtime ID of the arm state for one facing.
fn arm_id(facing: u8) -> u32 {
    10 + u32::from(facing)
}

/// Runtime ID of the piston body state for one facing.
fn body_id(facing: u8) -> u32 {
    20 + u32::from(facing)
}

/// Registers a full-cube floor, upward and eastward full-cube piston bodies, and arm states
/// with no static collision, as compiled for every piston arm, carrying their base, rod and
/// head pieces.
fn registry() -> CollisionRegistry {
    let mut registry = CollisionRegistry::new();
    registry.register(AIR, []).unwrap();
    registry
        .register(BODY, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    for facing in [UP, EAST] {
        let id = body_id(facing);
        registry
            .register(id, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
            .unwrap();
        assert!(registry.set_compiled_selection(
            id,
            &compiled(
                facing,
                SelectionContext::Static,
                [ShapeBox::new([0.0; 3], [1.0; 3])].into()
            )
        ));
    }
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
            &compiled(
                facing,
                SelectionContext::PistonArm,
                pieces.map(|(min, max)| ShapeBox::new(min, max)).into()
            )
        ));
    }
    registry
}

/// Compiled shapes of a piston body or arm state facing `facing`.
fn compiled(
    facing: u8,
    selection_context: SelectionContext,
    selection: Box<[ShapeBox]>,
) -> CompiledBlockShapes {
    CompiledBlockShapes {
        state_identity: [selection_context as u8 * 8 + facing; 32],
        actor_facing: Some(facing),
        actor_visual: None,
        selection,
        selection_context,
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

/// Places a piston body and its arm, with the body's actor reporting `state` when present.
fn piston(store: &mut ChunkStore, body: [i32; 3], facing: u8, state: Option<i8>) {
    let direction = assets::block_shapes::PISTON_DIRECTIONS[usize::from(facing)];
    set_block(store, body, body_id(facing));
    set_block(
        store,
        std::array::from_fn(|axis| body[axis] + direction[axis]),
        arm_id(facing),
    );
    if let Some(state) = state {
        report_state(store, body, state);
    }
}

/// Stores the piston body's actor reporting `state`, as a block-entity update delivers it.
fn report_state(store: &mut ChunkStore, [x, y, z]: [i32; 3], state: i8) {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("PistonArm".into()));
    root.insert("State", NbtValue::Byte(state));
    let actor = BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0;
    store
        .commit_block_entity_update(BlockEntityKey::new(0, x, y, z), actor)
        .unwrap();
}

/// Simulates `ticks` client ticks, which move every arm toward its reported state.
fn advance(store: &mut ChunkStore, ticks: u64) {
    store
        .set_simulation_tick(store.simulation_tick() + ticks)
        .unwrap();
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
    for (state, expected) in [(None, 5.0), (Some(EXPANDED), 6.0), (Some(RETRACTED), 5.0)] {
        let mut store = store();
        piston(&mut store, [4, 4, 4], UP, state);
        let feet = landing_height(&store, &registry);
        assert!(
            (feet - expected).abs() < 1e-6,
            "state {state:?}: landed at {feet}, expected {expected}"
        );
    }
}

#[test]
fn walking_into_a_sideways_arm_stops_at_its_head() {
    let registry = registry();
    let mut store = store();
    piston(&mut store, [4, 4, 4], EAST, Some(EXPANDED));
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
    piston(&mut store, [15, 4, 4], EAST, Some(EXPANDED));
    advance(&mut store, 2);
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
    report_state(&mut store, [15, 4, 4], RETRACTING);
    let moved = PaletteWorld::new(&store, &registry, 0, Default::default())
        .collision_boxes_with_provenance(query)
        .unwrap();
    assert_ne!(moved.identity, boxes.identity);
}

#[test]
fn an_arm_whose_piston_column_is_unloaded_has_no_collision() {
    let registry = registry();
    let mut store = ChunkStore::new();
    store.mark_chunk_loaded(ChunkKey::new(0, 1, 0)).unwrap();
    // The eastward arm at x = 16 is loaded; its piston's actor at x = 15 arrives before
    // that column's terrain.
    set_block(&mut store, [16, 4, 4], arm_id(EAST));
    report_state(&mut store, [15, 4, 4], EXPANDED);
    advance(&mut store, 2);
    let query = Aabb::new(Vec3::new(16.5, 4.0, 4.0), Vec3::new(17.5, 5.0, 5.0));
    let arm_boxes = |store: &ChunkStore| {
        PaletteWorld::for_movement(store, &registry, 0)
            .collision_boxes_with_provenance(query)
            .unwrap()
            .value
            .into_iter()
            .filter(|collider| collider.block == Some([16, 4, 4]))
            .count()
    };
    assert_eq!(arm_boxes(&store), 0);
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    assert!(arm_boxes(&store) > 0);
}

/// Native collision scans reach one cell past the query, so a retracted arm's sleeve that
/// pokes 0.25 past its piston is not found from the cell beyond the piston.
#[test]
fn arm_pieces_are_found_only_within_the_native_one_cell_scan() {
    let registry = registry();
    let mut store = store();
    piston(&mut store, [4, 4, 4], EAST, Some(RETRACTED));
    let arm_boxes = |query: Aabb| {
        PaletteWorld::for_movement(&store, &registry, 0)
            .collision_boxes_with_provenance(query)
            .unwrap()
            .value
            .into_iter()
            .filter(|collider| collider.block == Some([5, 4, 4]))
            .count()
    };
    let beyond = Aabb::new(Vec3::new(3.0, 4.0, 4.0), Vec3::new(3.9, 5.0, 5.0));
    assert_eq!(arm_boxes(beyond), 0);
    let within = Aabb::new(Vec3::new(3.0, 4.0, 4.0), Vec3::new(4.1, 5.0, 5.0));
    assert!(arm_boxes(within) > 0);
}

/// The highest x of the colliders the eastward arm cell at [5, 4, 4] contributes in `world`.
fn reach(world: &dyn CollisionWorld) -> f64 {
    world
        .collision_boxes_with_provenance(Aabb::new(
            Vec3::new(4.0, 4.0, 4.0),
            Vec3::new(7.0, 5.0, 5.0),
        ))
        .unwrap()
        .value
        .into_iter()
        .filter(|collider| collider.block == Some([5, 4, 4]))
        .map(|collider| collider.aabb.max.x)
        .fold(f64::MIN, f64::max)
}

/// [`reach`] in the live world after `tick`.
fn arm_reach(store: &ChunkStore, registry: &CollisionRegistry, tick: u64) -> f64 {
    let world = PaletteWorld::for_movement(store, registry, 0);
    world.begin_tick(tick);
    reach(&world)
}

/// One retracting report moves the arm's collision half a block per simulated tick, as the
/// native client steps its own progress, rather than holding the progress the server reported.
#[test]
fn a_retracting_arm_collides_where_each_tick_leaves_it() {
    let registry = registry();
    let mut store = store();
    piston(&mut store, [4, 4, 4], EAST, Some(EXPANDED));
    advance(&mut store, 2);
    report_state(&mut store, [4, 4, 4], RETRACTING);
    let tick = store.simulation_tick();
    let reach: Vec<f64> = (tick..=tick + 3)
        .map(|tick| arm_reach(&store, &registry, tick))
        .collect();
    assert_eq!(reach, [6.0, 5.5, 5.0, 5.0]);
}

/// Lets a player standing at `feet` settle, then reports `state` for the piston at
/// [4, 4, 4] at the player's tick and returns the player's feet after each of `ticks` ticks.
fn ride(
    store: &mut ChunkStore,
    registry: &CollisionRegistry,
    feet: Vec3,
    state: i8,
    ticks: usize,
) -> Vec<Vec3> {
    let mut player = PlayerState::new(feet);
    {
        let world = PaletteWorld::for_movement(store, registry, 0);
        for _ in 0..3 {
            Simulator::default()
                .tick(&mut player, MovementInput::default(), &world)
                .unwrap();
        }
    }
    assert!(
        player.on_ground,
        "the player settled at {:?}",
        player.position
    );
    store.set_simulation_tick(player.tick).unwrap();
    report_state(store, [4, 4, 4], state);
    let world = PaletteWorld::for_movement(store, registry, 0);
    (0..ticks)
        .map(|_| {
            Simulator::default()
                .tick(&mut player, MovementInput::default(), &world)
                .unwrap();
            player.position
        })
        .collect()
}

/// An extending upward arm lifts a player standing on its piston half a block per tick, as
/// native `PistonBlockActor::_moveCollidedEntities` pushes actors out of the arm's cube.
#[test]
fn an_extending_arm_lifts_the_player_standing_on_it() {
    let registry = registry();
    let mut store = store();
    piston(&mut store, [4, 4, 4], UP, Some(RETRACTED));
    let feet: Vec<f64> = ride(
        &mut store,
        &registry,
        Vec3::new(4.5, 5.0, 4.5),
        EXPANDING,
        4,
    )
    .into_iter()
    .map(|position| position.y)
    .collect();
    for (tick, (feet, expected)) in feet.iter().zip([5.5, 6.0, 6.0, 6.0]).enumerate() {
        assert!(
            (feet - expected).abs() < 1e-5,
            "tick {tick}: feet at {feet}, expected {expected}"
        );
    }
}

/// An extending sideways arm pushes a player beside it out along its facing.
#[test]
fn an_extending_sideways_arm_pushes_the_player_along_its_facing() {
    let registry = registry();
    let mut store = store();
    for x in 2..9 {
        set_block(&mut store, [x, 3, 4], BODY);
    }
    piston(&mut store, [4, 4, 4], EAST, Some(RETRACTED));
    let positions = ride(
        &mut store,
        &registry,
        Vec3::new(5.3, 4.0, 4.5),
        EXPANDING,
        3,
    );
    let trailing = Aabb::player_at(*positions.last().unwrap()).min.x;
    assert!(
        (trailing - 6.0).abs() < 1e-5,
        "player's trailing face at {trailing}"
    );
    assert!(positions.iter().all(|position| position.y == 4.0));
}

/// Replay shows each retained tick's arm progress, so a correction reproduces the pushes
/// that live prediction applied even after the world's clock moved on.
#[test]
fn replay_reproduces_the_pushes_of_each_retained_tick() {
    let registry = registry();
    let mut store = store();
    for x in 2..9 {
        set_block(&mut store, [x, 3, 4], BODY);
    }
    piston(&mut store, [4, 4, 4], EAST, Some(RETRACTED));
    let mut player = PlayerState::new(Vec3::new(5.3, 4.0, 4.5));
    {
        let world = PaletteWorld::for_movement(&store, &registry, 0);
        for _ in 0..3 {
            Simulator::default()
                .tick(&mut player, MovementInput::default(), &world)
                .unwrap();
        }
    }
    store.set_simulation_tick(player.tick).unwrap();
    report_state(&mut store, [4, 4, 4], EXPANDING);
    let mut history = PredictionHistory::new(8).unwrap();
    let predicted: Vec<PlayerState> = {
        let world = PaletteWorld::for_movement(&store, &registry, 0);
        (0..3)
            .map(|_| {
                history
                    .predict(
                        &mut player,
                        MovementInput::default(),
                        &Simulator::default(),
                        &world,
                    )
                    .unwrap();
                player.clone()
            })
            .collect()
    };
    store.set_simulation_tick(player.tick).unwrap();
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let mut current = player.clone();
    history
        .rewind_and_replay(
            &mut current,
            predicted[0].clone(),
            &Simulator::default(),
            &world,
        )
        .unwrap();
    assert_eq!(current.position, predicted[2].position);
    let retained: Vec<f64> = predicted
        .iter()
        .map(|state| reach(history.world_at(state.tick).unwrap()))
        .collect();
    assert_eq!(retained, [5.5, 6.0, 6.0]);
    let trailing = Aabb::player_at(current.position).min.x;
    assert!(
        (trailing - 6.0).abs() < 1e-5,
        "player's trailing face at {trailing}"
    );
}
