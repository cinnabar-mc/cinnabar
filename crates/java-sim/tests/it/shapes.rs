//! Block collision boxes and material facts: hand-checked shapes, comparison against fixtures
//! recorded from the 1.7.10 game classes, and a no-panic sweep over every id and metadata.

use std::collections::HashMap;
use std::fmt::Write as _;

use java_sim::block::{Material, collision_boxes, facts, id};
use java_sim::{Aabb, Block, World};
use serde_json::Value;

/// A sparse block world; every column is loaded and unset positions are air.
#[derive(Default)]
struct MapWorld {
    blocks: HashMap<(i32, i32, i32), Block>,
}

impl MapWorld {
    fn with(mut self, x: i32, y: i32, z: i32, id: u16, meta: u8) -> Self {
        self.blocks.insert((x, y, z), Block::new(id, meta));
        self
    }
}

impl World for MapWorld {
    fn block(&self, x: i32, y: i32, z: i32) -> Block {
        self.blocks.get(&(x, y, z)).copied().unwrap_or(Block::AIR)
    }

    fn chunk_loaded(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
        true
    }
}

/// Position of the block under test in the hand-written checks.
const X: i32 = 3;
const Y: i32 = 64;
const Z: i32 = -7;

/// A mask that intersects every box a block near the origin can produce.
const EVERYWHERE: Aabb = Aabb::new(-1e6, -1e6, -1e6, 1e6, 1e6, 1e6);

/// A world holding one block at the test position.
fn lone(id: u16, meta: u8) -> MapWorld {
    MapWorld::default().with(X, Y, Z, id, meta)
}

/// The collision boxes of the block at `(x, y, z)` against a mask that keeps them all.
fn boxes_at(world: &MapWorld, x: i32, y: i32, z: i32) -> Vec<Aabb> {
    boxes_within(world, x, y, z, &EVERYWHERE)
}

/// The collision boxes of the block at `(x, y, z)` that intersect `mask`.
fn boxes_within(world: &MapWorld, x: i32, y: i32, z: i32, mask: &Aabb) -> Vec<Aabb> {
    let mut boxes = Vec::new();
    collision_boxes(world, x, y, z, mask, &mut boxes);
    boxes
}

fn bits(aabb: &Aabb) -> [u64; 6] {
    [
        aabb.min_x, aabb.min_y, aabb.min_z, aabb.max_x, aabb.max_y, aabb.max_z,
    ]
    .map(f64::to_bits)
}

/// A box given as block-local corners, moved to the test position.
fn local(corners: [f64; 6]) -> Aabb {
    let [min_x, min_y, min_z, max_x, max_y, max_z] = corners;
    Aabb::new(
        f64::from(X) + min_x,
        f64::from(Y) + min_y,
        f64::from(Z) + min_z,
        f64::from(X) + max_x,
        f64::from(Y) + max_y,
        f64::from(Z) + max_z,
    )
}

/// Asserts the block at the test position has exactly `expected` block-local boxes, in order and
/// bit for bit.
#[track_caller]
fn assert_shape(world: &MapWorld, expected: &[[f64; 6]]) {
    let actual = boxes_at(world, X, Y, Z);
    let expected: Vec<Aabb> = expected.iter().copied().map(local).collect();
    assert_eq!(
        actual.iter().map(bits).collect::<Vec<_>>(),
        expected.iter().map(bits).collect::<Vec<_>>(),
        "boxes {actual:?} differ from {expected:?}"
    );
}

const STONE: u16 = 1;
const UNIT: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

#[test]
fn cubes_and_empty_blocks() {
    for id in [1, 2, 7, 18, 20, 52, 79, 89, 138, 152, 174] {
        assert_shape(&lone(id, 0), &[UNIT]);
    }
    // Air, liquids, plants, torches, signs, rails, plates, buttons, portals, moving pistons and
    // ids the registry does not hold.
    for id in [
        0, 6, 8, 9, 10, 11, 30, 31, 36, 50, 63, 66, 70, 77, 83, 90, 106, 119, 132, 165, 169, 176,
        4095,
    ] {
        assert_shape(&lone(id, 0), &[]);
    }
}

#[test]
fn slabs_follow_the_top_bit() {
    for id in [44, 126] {
        assert_shape(&lone(id, 0), &[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]);
        assert_shape(&lone(id, 3), &[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]);
        assert_shape(&lone(id, 8), &[[0.0, 0.5, 0.0, 1.0, 1.0, 1.0]]);
    }
    for id in [43, 125] {
        assert_shape(&lone(id, 8), &[UNIT]);
    }
}

#[test]
fn straight_stairs_are_a_slab_and_a_step() {
    let slab = [0.0, 0.0, 0.0, 1.0, 0.5, 1.0];
    assert_shape(&lone(53, 0), &[slab, [0.5, 0.5, 0.0, 1.0, 1.0, 1.0]]);
    assert_shape(&lone(53, 1), &[slab, [0.0, 0.5, 0.0, 0.5, 1.0, 1.0]]);
    assert_shape(&lone(53, 2), &[slab, [0.0, 0.5, 0.5, 1.0, 1.0, 1.0]]);
    assert_shape(&lone(53, 3), &[slab, [0.0, 0.5, 0.0, 1.0, 1.0, 0.5]]);
    // Upside down: the slab is the top half and the step hangs below it.
    assert_shape(
        &lone(67, 4),
        &[
            [0.0, 0.5, 0.0, 1.0, 1.0, 1.0],
            [0.5, 0.0, 0.0, 1.0, 0.5, 1.0],
        ],
    );
}

#[test]
fn stairs_form_outer_and_inner_corners() {
    let slab = [0.0, 0.0, 0.0, 1.0, 0.5, 1.0];
    // A north-rising stair in front of an east-rising one cuts the step to its north quarter.
    let outer = lone(53, 0).with(X + 1, Y, Z, 109, 3);
    assert_shape(&outer, &[slab, [0.5, 0.5, 0.0, 1.0, 1.0, 0.5]]);
    // A south-rising stair in front cuts it to the south quarter.
    let outer_south = lone(53, 0).with(X + 1, Y, Z, 109, 2);
    assert_shape(&outer_south, &[slab, [0.5, 0.5, 0.5, 1.0, 1.0, 1.0]]);
    // A twin on the corner's side keeps the stair straight.
    let blocked = lone(53, 0)
        .with(X + 1, Y, Z, 109, 3)
        .with(X, Y, Z + 1, 53, 0);
    assert_shape(&blocked, &[slab, [0.5, 0.5, 0.0, 1.0, 1.0, 1.0]]);
    // A stair in the other half does not form a corner.
    let other_half = lone(53, 0).with(X + 1, Y, Z, 109, 7);
    assert_shape(&other_half, &[slab, [0.5, 0.5, 0.0, 1.0, 1.0, 1.0]]);

    // A north-rising stair behind an east-rising one adds the north-west quarter.
    let inner = lone(53, 0).with(X - 1, Y, Z, 109, 3);
    assert_shape(
        &inner,
        &[
            slab,
            [0.5, 0.5, 0.0, 1.0, 1.0, 1.0],
            [0.0, 0.5, 0.0, 0.5, 1.0, 0.5],
        ],
    );
    // Behind a south-rising stair, an east-rising one to the north adds the north-east quarter.
    let inner_south = lone(53, 2).with(X, Y, Z - 1, 53, 0);
    assert_shape(
        &inner_south,
        &[
            slab,
            [0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            [0.5, 0.5, 0.0, 1.0, 1.0, 0.5],
        ],
    );
}

#[test]
fn fences_join_neighbours_with_tall_bars() {
    let post = [0.375, 0.0, 0.375, 0.625, 1.5, 0.625];
    assert_shape(&lone(85, 0), &[post]);
    // North and south connections make the Z bar; without west or east there is no X bar.
    let north = lone(85, 0).with(X, Y, Z - 1, STONE, 0);
    assert_shape(&north, &[[0.375, 0.0, 0.0, 0.625, 1.5, 0.625]]);
    // The Z bar comes first, then the X bar.
    let corner = lone(85, 0)
        .with(X, Y, Z + 1, 85, 0)
        .with(X - 1, Y, Z, 107, 0);
    assert_shape(
        &corner,
        &[
            [0.375, 0.0, 0.375, 0.625, 1.5, 1.0],
            [0.0, 0.0, 0.375, 0.625, 1.5, 0.625],
        ],
    );
    let east = lone(113, 0).with(X + 1, Y, Z, 113, 0);
    assert_shape(&east, &[[0.375, 0.0, 0.375, 1.0, 1.5, 0.625]]);
}

#[test]
fn fences_connect_to_opaque_cubes_but_not_gourds_or_other_fences() {
    let connects = |own: u16, neighbour: u16| {
        let world = lone(own, 0).with(X + 1, Y, Z, neighbour, 0);
        boxes_at(&world, X, Y, Z)[0].max_x == f64::from(X) + 1.0
    };
    // Redstone blocks provide power, so they are not normal cubes, but fences still join them.
    for neighbour in [1, 5, 152, 107, 85, 43, 120, 52] {
        assert!(
            connects(85, neighbour),
            "fence should connect to {neighbour}"
        );
    }
    for neighbour in [0, 86, 91, 103, 113, 20, 18, 89, 79, 44, 53, 139, 46] {
        assert!(
            !connects(85, neighbour),
            "fence should not connect to {neighbour}"
        );
    }
    assert!(connects(139, 139) && connects(139, 107) && connects(139, 1));
    assert!(!connects(139, 85) && !connects(139, 103));
}

#[test]
fn walls_span_their_arms_in_one_tall_box() {
    assert_shape(&lone(139, 0), &[[0.25, 0.0, 0.25, 0.75, 1.5, 0.75]]);
    let straight = lone(139, 0)
        .with(X, Y, Z - 1, 139, 0)
        .with(X, Y, Z + 1, 139, 1);
    assert_shape(&straight, &[[0.3125, 0.0, 0.0, 0.6875, 1.5, 1.0]]);
    let straight_x = lone(139, 0)
        .with(X - 1, Y, Z, STONE, 0)
        .with(X + 1, Y, Z, 107, 0);
    assert_shape(&straight_x, &[[0.0, 0.0, 0.3125, 1.0, 1.5, 0.6875]]);
    let corner = lone(139, 0)
        .with(X, Y, Z - 1, 139, 0)
        .with(X + 1, Y, Z, 139, 0);
    assert_shape(&corner, &[[0.25, 0.0, 0.0, 1.0, 1.5, 0.75]]);
}

#[test]
fn panes_build_a_strip_per_axis() {
    let along_x = [0.0, 0.0, 0.4375, 1.0, 1.0, 0.5625];
    let along_z = [0.4375, 0.0, 0.0, 0.5625, 1.0, 1.0];
    for id in [101, 102, 160] {
        assert_shape(&lone(id, 0), &[along_x, along_z]);
    }
    let west = lone(102, 0).with(X - 1, Y, Z, STONE, 0);
    assert_shape(&west, &[[0.0, 0.0, 0.4375, 0.5, 1.0, 0.5625]]);
    let east_south = lone(102, 0)
        .with(X + 1, Y, Z, 101, 0)
        .with(X, Y, Z + 1, 20, 0);
    assert_shape(
        &east_south,
        &[
            [0.5, 0.0, 0.4375, 1.0, 1.0, 0.5625],
            [0.4375, 0.0, 0.5, 0.5625, 1.0, 1.0],
        ],
    );
    let north_south = lone(101, 0)
        .with(X, Y, Z - 1, 95, 0)
        .with(X, Y, Z + 1, 160, 5);
    assert_shape(&north_south, &[along_z]);
    // Leaves and glowstone count as full blocks for panes; single slabs and fences do not.
    for (neighbour, joins) in [
        (18, true),
        (161, true),
        (89, true),
        (43, true),
        (44, false),
        (85, false),
        (79, false),
    ] {
        let world = lone(102, 0).with(X, Y, Z - 1, neighbour, 0);
        let expected: &[[f64; 6]] = if joins {
            &[[0.4375, 0.0, 0.0, 0.5625, 1.0, 0.5]]
        } else {
            &[along_x, along_z]
        };
        assert_shape(&world, expected);
    }
}

#[test]
fn position_summed_shapes() {
    assert_shape(
        &lone(81, 0),
        &[[0.0625, 0.0, 0.0625, 0.9375, 0.9375, 0.9375]],
    );
    assert_shape(&lone(88, 0), &[[0.0, 0.0, 0.0, 1.0, 0.875, 1.0]]);
    assert_shape(&lone(60, 7), &[UNIT]);
    assert_shape(&lone(171, 5), &[[0.0, 0.0, 0.0, 1.0, 0.0, 1.0]]);
    for meta in 0..16_u8 {
        let layers = f64::from(meta & 7);
        assert_shape(
            &lone(78, meta),
            &[[0.0, 0.0, 0.0, 1.0, layers * 0.125, 1.0]],
        );
    }
    for bites in 0..=6_u8 {
        let eaten = f64::from(1 + bites * 2) / 16.0;
        assert_shape(
            &lone(92, bites),
            &[[eaten, 0.0, 0.0625, 0.9375, 0.4375, 0.9375]],
        );
    }
}

#[test]
fn position_sums_round_in_single_precision() {
    // Odd integers this large are not `f32` values: the game's float sums round the block
    // position to an even neighbour and lose the sixteenth.
    let x = 20_000_001;
    let far = Aabb::new(-1e9, -1e9, -1e9, 1e9, 1e9, 1e9);
    let cactus = MapWorld::default().with(x, Y, Z, 81, 0);
    let boxes = boxes_within(&cactus, x, Y, Z, &far);
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].min_x.to_bits(), 20_000_000.0_f64.to_bits());
    assert_eq!(boxes[0].max_x.to_bits(), 20_000_002.0_f64.to_bits());
    assert_eq!(boxes[0].min_z.to_bits(), (-6.9375_f64).to_bits());

    let gate = MapWorld::default().with(x, Y, Z, 107, 1);
    // Both faces of the quarter-thick gate bar round onto the same value.
    let boxes = boxes_within(&gate, x, Y, Z, &far);
    assert_eq!(boxes[0].min_x.to_bits(), 20_000_000.0_f64.to_bits());
    assert_eq!(boxes[0].max_x.to_bits(), 20_000_000.0_f64.to_bits());

    // Bounds-based shapes add the position in double precision and keep it.
    let slab = MapWorld::default().with(x, Y, Z, 44, 0);
    assert_eq!(
        boxes_within(&slab, x, Y, Z, &far)[0].min_x.to_bits(),
        20_000_001.0_f64.to_bits()
    );
}

#[test]
fn chests_reach_towards_the_first_matching_neighbour() {
    let single = [0.0625, 0.0, 0.0625, 0.9375, 0.875, 0.9375];
    assert_shape(&lone(54, 0), &[single]);
    assert_shape(&lone(130, 0), &[single]);
    assert_shape(
        &lone(54, 0).with(X, Y, Z - 1, 54, 0),
        &[[0.0625, 0.0, 0.0, 0.9375, 0.875, 0.9375]],
    );
    assert_shape(
        &lone(54, 0).with(X, Y, Z + 1, 54, 0),
        &[[0.0625, 0.0, 0.0625, 0.9375, 0.875, 1.0]],
    );
    assert_shape(
        &lone(146, 0).with(X - 1, Y, Z, 146, 0),
        &[[0.0, 0.0, 0.0625, 0.9375, 0.875, 0.9375]],
    );
    assert_shape(
        &lone(146, 0).with(X + 1, Y, Z, 146, 0),
        &[[0.0625, 0.0, 0.0625, 1.0, 0.875, 0.9375]],
    );
    // A trapped chest is a different block, and ender chests never join.
    assert_shape(&lone(54, 0).with(X + 1, Y, Z, 146, 0), &[single]);
    assert_shape(&lone(130, 0).with(X + 1, Y, Z, 130, 0), &[single]);
}

#[test]
fn ladders_collide_with_a_thin_slab_on_their_wall() {
    assert_shape(&lone(65, 2), &[[0.0, 0.0, 0.875, 1.0, 1.0, 1.0]]);
    assert_shape(&lone(65, 3), &[[0.0, 0.0, 0.0, 1.0, 1.0, 0.125]]);
    assert_shape(&lone(65, 4), &[[0.875, 0.0, 0.0, 1.0, 1.0, 1.0]]);
    assert_shape(&lone(65, 5), &[[0.0, 0.0, 0.0, 0.125, 1.0, 1.0]]);
}

#[test]
fn trapdoors_lie_flat_or_stand_against_a_side() {
    assert_shape(&lone(96, 0), &[[0.0, 0.0, 0.0, 1.0, 0.1875, 1.0]]);
    assert_shape(&lone(96, 8), &[[0.0, 0.8125, 0.0, 1.0, 1.0, 1.0]]);
    assert_shape(&lone(96, 4), &[[0.0, 0.0, 0.8125, 1.0, 1.0, 1.0]]);
    assert_shape(&lone(96, 5), &[[0.0, 0.0, 0.0, 1.0, 1.0, 0.1875]]);
    assert_shape(&lone(96, 6), &[[0.8125, 0.0, 0.0, 1.0, 1.0, 1.0]]);
    assert_shape(&lone(96, 15), &[[0.0, 0.0, 0.0, 0.1875, 1.0, 1.0]]);
}

#[test]
fn door_halves_share_facing_and_hinge() {
    let west = [0.0, 0.0, 0.0, 0.1875, 1.0, 1.0];
    let north = [0.0, 0.0, 0.0, 1.0, 1.0, 0.1875];
    let east = [0.8125, 0.0, 0.0, 1.0, 1.0, 1.0];
    let south = [0.0, 0.0, 0.8125, 1.0, 1.0, 1.0];
    // (lower metadata, upper metadata, expected leaf) for both halves.
    let cases = [
        (0, 8, west),
        (1, 8, north),
        (2, 8, east),
        (3, 8, south),
        (4, 8, north),
        (4, 9, south),
        (5, 8, east),
        (5, 9, west),
        (6, 8, south),
        (6, 9, north),
        (7, 8, west),
        (7, 9, east),
    ];
    for id in [64, 71] {
        for (lower, upper, leaf) in cases {
            let world = lone(id, lower).with(X, Y + 1, Z, id, upper);
            assert_shape(&world, &[leaf]);
            let top = boxes_at(&world, X, Y + 1, Z);
            let mut expected = local(leaf);
            expected.offset(0.0, 1.0, 0.0);
            assert_eq!(top.iter().map(bits).collect::<Vec<_>>(), [bits(&expected)]);
        }
    }
}

#[test]
fn fence_gates_block_only_when_closed() {
    assert_shape(&lone(107, 0), &[[0.0, 0.0, 0.375, 1.0, 1.5, 0.625]]);
    assert_shape(&lone(107, 2), &[[0.0, 0.0, 0.375, 1.0, 1.5, 0.625]]);
    assert_shape(&lone(107, 1), &[[0.375, 0.0, 0.0, 0.625, 1.5, 1.0]]);
    assert_shape(&lone(107, 3), &[[0.375, 0.0, 0.0, 0.625, 1.5, 1.0]]);
    for open in 4..8 {
        assert_shape(&lone(107, open), &[]);
    }
}

#[test]
fn fixed_low_shapes() {
    assert_shape(&lone(111, 0), &[[0.0, 0.0, 0.0, 1.0, 0.015625, 1.0]]);
    assert_shape(&lone(26, 10), &[[0.0, 0.0, 0.0, 1.0, 0.5625, 1.0]]);
    assert_shape(&lone(116, 0), &[[0.0, 0.0, 0.0, 1.0, 0.75, 1.0]]);
    assert_shape(&lone(151, 0), &[[0.0, 0.0, 0.0, 1.0, 0.375, 1.0]]);
    assert_shape(
        &lone(140, 3),
        &[[0.3125, 0.0, 0.3125, 0.6875, 0.375, 0.6875]],
    );
    assert_shape(&lone(122, 0), &[[0.0625, 0.0, 0.0625, 0.9375, 1.0, 0.9375]]);
    for diode in [93, 94, 149, 150] {
        assert_shape(&lone(diode, 0), &[[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]]);
    }
}

#[test]
fn anvils_are_narrow_across_their_axis() {
    for meta in [0, 2, 4, 8] {
        assert_shape(&lone(145, meta), &[[0.125, 0.0, 0.0, 0.875, 1.0, 1.0]]);
    }
    for meta in [1, 3, 5, 11] {
        assert_shape(&lone(145, meta), &[[0.0, 0.0, 0.125, 1.0, 1.0, 0.875]]);
    }
}

#[test]
fn skulls_sit_on_the_floor_or_a_wall() {
    assert_shape(&lone(144, 1), &[[0.25, 0.0, 0.25, 0.75, 0.5, 0.75]]);
    assert_shape(&lone(144, 2), &[[0.25, 0.25, 0.5, 0.75, 0.75, 1.0]]);
    assert_shape(&lone(144, 3), &[[0.25, 0.25, 0.0, 0.75, 0.75, 0.5]]);
    assert_shape(&lone(144, 4), &[[0.5, 0.25, 0.25, 1.0, 0.75, 0.75]]);
    assert_shape(&lone(144, 5), &[[0.0, 0.25, 0.25, 0.5, 0.75, 0.75]]);
}

#[test]
fn cocoa_pods_grow_from_their_side() {
    // Age 0 is 4 wide and 5 tall; age 2 is 8 wide and 9 tall.
    assert_shape(
        &lone(127, 0),
        &[[0.375, 0.4375, 0.6875, 0.625, 0.75, 0.9375]],
    );
    assert_shape(
        &lone(127, 1),
        &[[0.0625, 0.4375, 0.375, 0.3125, 0.75, 0.625]],
    );
    assert_shape(
        &lone(127, 2),
        &[[0.375, 0.4375, 0.0625, 0.625, 0.75, 0.3125]],
    );
    assert_shape(
        &lone(127, 3),
        &[[0.6875, 0.4375, 0.375, 0.9375, 0.75, 0.625]],
    );
    assert_shape(&lone(127, 8), &[[0.25, 0.1875, 0.4375, 0.75, 0.75, 0.9375]]);
}

#[test]
fn basins_are_a_floor_and_four_walls() {
    let walls = [
        [0.0, 0.0, 0.0, 0.125, 1.0, 1.0],
        [0.0, 0.0, 0.0, 1.0, 1.0, 0.125],
        [0.875, 0.0, 0.0, 1.0, 1.0, 1.0],
        [0.0, 0.0, 0.875, 1.0, 1.0, 1.0],
    ];
    let with_floor = |height: f64| {
        let mut boxes = vec![[0.0, 0.0, 0.0, 1.0, height, 1.0]];
        boxes.extend(walls);
        boxes
    };
    assert_shape(&lone(118, 2), &with_floor(0.3125));
    assert_shape(&lone(154, 3), &with_floor(0.625));
}

#[test]
fn brewing_stand_and_portal_frame() {
    assert_shape(
        &lone(117, 0),
        &[
            [0.4375, 0.0, 0.4375, 0.5625, 0.875, 0.5625],
            [0.0, 0.0, 0.0, 1.0, 0.125, 1.0],
        ],
    );
    assert_shape(&lone(120, 2), &[[0.0, 0.0, 0.0, 1.0, 0.8125, 1.0]]);
    assert_shape(
        &lone(120, 6),
        &[
            [0.0, 0.0, 0.0, 1.0, 0.8125, 1.0],
            [0.3125, 0.8125, 0.3125, 0.6875, 1.0, 0.6875],
        ],
    );
}

#[test]
fn pistons_shrink_when_extended_and_heads_have_an_arm() {
    for id in [29, 33] {
        assert_shape(&lone(id, 1), &[UNIT]);
        assert_shape(&lone(id, 8), &[[0.0, 0.25, 0.0, 1.0, 1.0, 1.0]]);
        assert_shape(&lone(id, 9), &[[0.0, 0.0, 0.0, 1.0, 0.75, 1.0]]);
        assert_shape(&lone(id, 10), &[[0.0, 0.0, 0.25, 1.0, 1.0, 1.0]]);
        assert_shape(&lone(id, 11), &[[0.0, 0.0, 0.0, 1.0, 1.0, 0.75]]);
        assert_shape(&lone(id, 12), &[[0.25, 0.0, 0.0, 1.0, 1.0, 1.0]]);
        assert_shape(&lone(id, 13), &[[0.0, 0.0, 0.0, 0.75, 1.0, 1.0]]);
        assert_shape(&lone(id, 14), &[UNIT]);
    }
    assert_shape(
        &lone(34, 0),
        &[
            [0.0, 0.0, 0.0, 1.0, 0.25, 1.0],
            [0.375, 0.25, 0.375, 0.625, 1.0, 0.625],
        ],
    );
    assert_shape(
        &lone(34, 9),
        &[
            [0.0, 0.75, 0.0, 1.0, 1.0, 1.0],
            [0.375, 0.0, 0.375, 0.625, 0.75, 0.625],
        ],
    );
    assert_shape(
        &lone(34, 2),
        &[
            [0.0, 0.0, 0.0, 1.0, 1.0, 0.25],
            [0.25, 0.375, 0.25, 0.75, 0.625, 1.0],
        ],
    );
    assert_shape(
        &lone(34, 3),
        &[
            [0.0, 0.0, 0.75, 1.0, 1.0, 1.0],
            [0.25, 0.375, 0.0, 0.75, 0.625, 0.75],
        ],
    );
    assert_shape(
        &lone(34, 4),
        &[
            [0.0, 0.0, 0.0, 0.25, 1.0, 1.0],
            [0.375, 0.25, 0.25, 0.625, 0.75, 1.0],
        ],
    );
    let east = [
        [0.75, 0.0, 0.0, 1.0, 1.0, 1.0],
        [0.0, 0.375, 0.25, 0.75, 0.625, 0.75],
    ];
    assert_shape(&lone(34, 5), &east);
    // Orientations past the last side clamp to it.
    assert_shape(&lone(34, 7), &east);
}

#[test]
fn only_boxes_touching_the_mask_are_added() {
    let world = lone(53, 0);
    let mut boxes = vec![Aabb::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)];
    // Above the slab half: only the step is hit, and earlier entries stay.
    let above_slab = Aabb::new(
        f64::from(X),
        f64::from(Y) + 0.5,
        f64::from(Z),
        f64::from(X) + 1.0,
        f64::from(Y) + 2.0,
        f64::from(Z) + 1.0,
    );
    collision_boxes(&world, X, Y, Z, &above_slab, &mut boxes);
    assert_eq!(boxes.len(), 2);
    assert_eq!(
        bits(&boxes[1]),
        bits(&local([0.5, 0.5, 0.0, 1.0, 1.0, 1.0]))
    );
    // A mask that only touches the block's face adds nothing.
    let beside = Aabb::new(
        f64::from(X) + 1.0,
        f64::from(Y),
        f64::from(Z),
        f64::from(X) + 2.0,
        f64::from(Y) + 1.0,
        f64::from(Z) + 1.0,
    );
    collision_boxes(&world, X, Y, Z, &beside, &mut boxes);
    assert_eq!(boxes.len(), 2);
}

#[test]
fn facts_follow_material_and_class() {
    let stone = facts(1);
    assert!(
        stone.solid && stone.blocks_movement && stone.opaque && stone.normal_cube && !stone.liquid
    );
    assert_eq!(stone.slipperiness.to_bits(), 0.6_f32.to_bits());
    for id in [8, 9] {
        let water = facts(id);
        assert_eq!(water.material, Material::Water);
        assert!(water.liquid && !water.solid && !water.blocks_movement && !water.opaque);
    }
    assert_eq!(facts(11).material, Material::Lava);
    let web = facts(30);
    assert!(web.material == Material::Web && web.solid && !web.blocks_movement && !web.opaque);
    for id in [79, 174] {
        assert_eq!(facts(id).slipperiness.to_bits(), 0.98_f32.to_bits());
    }
    assert!(facts(79).material == Material::Ice && !facts(79).normal_cube);
    assert!(facts(174).normal_cube);
    // Opaque materials that are not normal cubes: partial blocks and the powered redstone block.
    for id in [44, 53, 60, 85, 152, 54, 116, 145] {
        assert!(facts(id).opaque && !facts(id).normal_cube, "id {id}");
    }
    // Translucent materials are never normal cubes, whatever their shape.
    for id in [18, 20, 46, 89, 81] {
        assert!(
            facts(id).blocks_movement && !facts(id).opaque && !facts(id).normal_cube,
            "id {id}"
        );
    }
    for id in [43, 52, 120, 125, 137] {
        assert!(facts(id).normal_cube, "id {id}");
    }
    for id in [0, 165, 169, 176, u16::MAX] {
        assert_eq!(facts(id), facts(0), "id {id}");
        assert_eq!(facts(id).material, Material::Air);
    }
}

#[test]
fn every_id_and_metadata_yields_ordered_boxes() {
    let neighbours = [
        (-1, 0, 0),
        (1, 0, 0),
        (0, -1, 0),
        (0, 1, 0),
        (0, 0, -1),
        (0, 0, 1),
    ];
    for id in 0..=255_u16 {
        for meta in 0..=15_u8 {
            let surrounded = |by: Block| {
                neighbours
                    .iter()
                    .fold(lone(id, meta), |world, (dx, dy, dz)| {
                        world.with(X + dx, Y + dy, Z + dz, by.id, by.meta)
                    })
            };
            let layouts = [
                lone(id, meta),
                surrounded(Block::new(STONE, 0)),
                surrounded(Block::new(id, meta)),
            ];
            for world in &layouts {
                for aabb in boxes_at(world, X, Y, Z) {
                    // A cake whose metadata counts more bites than it has slices recedes past
                    // its own east side: the game builds that box with its X extent inverted.
                    let overeaten_cake = id == 92 && meta >= 8;
                    assert_eq!(
                        aabb.min_x > aabb.max_x,
                        overeaten_cake,
                        "id {id} meta {meta}: {aabb:?}"
                    );
                    assert!(
                        aabb.min_y <= aabb.max_y && aabb.min_z <= aabb.max_z,
                        "id {id} meta {meta}: inverted box {aabb:?}"
                    );
                    assert!(
                        EVERYWHERE.intersects(&aabb),
                        "id {id} meta {meta}: {aabb:?} misses the mask"
                    );
                }
            }
        }
    }
}

/// The text of a fixture recorded from the game, or `None` when the file is not present.
fn fixture(name: &str) -> Option<String> {
    crate::fixtures::read(&crate::fixtures::directory().join(name))
}

/// Fails with the number of mismatches and the first twenty of them.
#[track_caller]
fn assert_no_mismatches(what: &str, mismatches: &[String]) {
    assert!(
        mismatches.is_empty(),
        "{} {what} mismatches, first {}:\n{}",
        mismatches.len(),
        mismatches.len().min(20),
        mismatches[..mismatches.len().min(20)].join("\n")
    );
}

#[test]
fn facts_match_the_game_registry() {
    let Some(lines) = fixture("blocks.jsonl.gz") else {
        eprintln!(
            "missing fixture: crates/java-sim/fixtures/blocks.jsonl.gz; skipping registry comparison"
        );
        return;
    };
    let mut mismatches = Vec::new();
    for line in lines.lines().filter(|line| !line.trim().is_empty()) {
        let row: Value = serde_json::from_str(line).expect("blocks.jsonl line is JSON");
        let id = u16::try_from(row["id"].as_u64().expect("id")).expect("id fits u16");
        let name = row["name"].as_str().unwrap_or("?");
        let actual = facts(id);
        let mut differences = String::new();
        let flags = [
            ("liquid", actual.liquid),
            ("solid", actual.solid),
            ("opaque", actual.opaque),
            ("blocks_movement", actual.blocks_movement),
            ("normal_cube", actual.normal_cube),
        ];
        for (key, ours) in flags {
            let theirs = row[key].as_bool().expect("boolean fact");
            if ours != theirs {
                write!(differences, " {key}: ours {ours}, game {theirs};")
                    .expect("write to string");
            }
        }
        let slipperiness = row["slipperiness"].as_i64().expect("slipperiness bits") as u32;
        if actual.slipperiness.to_bits() != slipperiness {
            let theirs = f32::from_bits(slipperiness);
            write!(
                differences,
                " slipperiness: ours {}, game {theirs};",
                actual.slipperiness
            )
            .expect("write to string");
        }
        let material = row["material"].as_str().expect("material name");
        let material = material.strip_prefix("Material.").unwrap_or(material);
        let named = [
            ("air", Material::Air),
            ("water", Material::Water),
            ("lava", Material::Lava),
            ("ice", Material::Ice),
            ("web", Material::Web),
        ];
        for (field, variant) in named {
            if (material == field) != (actual.material == variant) {
                write!(
                    differences,
                    " material: ours {:?}, game {material};",
                    actual.material
                )
                .expect("write to string");
            }
        }
        if !differences.is_empty() {
            mismatches.push(format!("id {id} ({name}):{differences}"));
        }
    }
    assert_no_mismatches("block fact", &mismatches);
}

/// Reads `[x, y, z]` from the front of a JSON array.
fn position(value: &Value) -> (i32, i32, i32) {
    let axis = |index: usize| {
        i32::try_from(value[index].as_i64().expect("coordinate")).expect("coordinate fits i32")
    };
    (axis(0), axis(1), axis(2))
}

fn describe(boxes: &[[u64; 6]]) -> String {
    let rows: Vec<String> = boxes
        .iter()
        .map(|corners| format!("{:?}", corners.map(f64::from_bits)))
        .collect();
    format!("[{}]", rows.join(", "))
}

#[test]
fn collision_boxes_match_the_game() {
    let Some(lines) = fixture("shapes.jsonl.gz") else {
        eprintln!(
            "missing fixture: crates/java-sim/fixtures/shapes.jsonl.gz; skipping shape comparison"
        );
        return;
    };
    let mut mismatches = Vec::new();
    let mut compared = 0_usize;
    for line in lines.lines().filter(|line| !line.trim().is_empty()) {
        let case: Value = serde_json::from_str(line).expect("shapes.jsonl line is JSON");
        let Some(expected) = case.get("boxes").and_then(Value::as_array) else {
            assert!(
                case.get("error").is_some(),
                "shape case has neither boxes nor error: {line}"
            );
            continue;
        };
        let mut world = MapWorld::default();
        for block in case["blocks"].as_array().expect("blocks") {
            let (x, y, z) = position(block);
            let id = u16::try_from(block[3].as_u64().expect("id")).expect("id fits u16");
            let meta = u8::try_from(block[4].as_u64().expect("meta")).expect("meta fits u8");
            world = world.with(x, y, z, id, meta);
        }
        let (x, y, z) = position(&case["at"]);
        let block = world.block(x, y, z);
        // A ladder whose metadata names no wall leaves the ladder bounds as the last ladder set
        // them, so the recorded boxes depend on the order the cases were generated in.
        if block.id == id::LADDER && !(2..=5).contains(&block.meta) {
            continue;
        }
        let expected: Vec<[u64; 6]> = expected
            .iter()
            .map(|corners| {
                std::array::from_fn(|index| corners[index].as_i64().expect("f64 bits") as u64)
            })
            .collect();
        let actual: Vec<[u64; 6]> = boxes_at(&world, x, y, z).iter().map(bits).collect();
        compared += 1;
        if actual != expected {
            mismatches.push(format!(
                "case {} id {} meta {} at ({x}, {y}, {z}) blocks {}:\n  ours {}\n  game {}",
                case["case"].as_str().unwrap_or("?"),
                block.id,
                block.meta,
                case["blocks"],
                describe(&actual),
                describe(&expected),
            ));
        }
    }
    assert!(compared > 0, "shapes.jsonl holds no comparable case");
    assert_no_mismatches("collision shape", &mismatches);
}
