//! Door and bed items place two cells through their own use rather than as block items.

use assets::{
    NetworkIdMode,
    facts::{BlockFact, SupportKind},
    registry::{BlockTraits as T, ModelFamily},
};

use super::{UseSurroundings, placement_cell};
use crate::{movement::PhysicsCollisionRegistries, placement_state::yaw_quadrant};

/// Native `BedItem::_tryUseOn` scans this many cells above and below the foot for another bed.
const BED_COLUMN_REACH: i32 = 3;

/// The default state of the door or bed block named by a non-block item, or `None` for every
/// other item. Their item identifiers name their blocks.
pub fn paired_item_block(
    collisions: &PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    identifier: &str,
) -> Option<u32> {
    let block = collisions.block_default_runtime_id(mode, identifier)?;
    let fact = collisions.block_facts(mode, block);
    (fact.traits.contains(T::DOOR) || fact.family == ModelFamily::Bed).then_some(block)
}

/// Whether a click has room for `placed`, the block of a door or bed item, with every cell
/// inside `build_height`. `read` returns the facts of a cell, and `None` when unreadable; this
/// refuses the use rather than guess.
///
/// Follows the cell choices of native `DoorItem::_useOn` and `BedItem::_tryUseOn`. Facts here
/// do not settle every support rule, so any non-empty support is accepted; the server's result
/// still decides the cells drawn.
pub fn paired_item_fits(
    placed: BlockFact,
    clicked: [i32; 3],
    face: u8,
    yaw: f32,
    surroundings: &UseSurroundings,
    build_height: std::ops::Range<i32>,
    read: impl Fn([i32; 3]) -> Option<BlockFact>,
) -> bool {
    let (foot, free) = surroundings.destination(clicked, face);
    if !free || !build_height.contains(&foot[1]) {
        return false;
    }
    let replaceable = |cell| read(cell).is_some_and(|fact| fact.traits.contains(T::REPLACEABLE));
    let supported = |cell: [i32; 3]| {
        build_height.contains(&(cell[1] - 1))
            && read([cell[0], cell[1] - 1, cell[2]]).is_some_and(|fact| {
                !fact.traits.contains(T::AIR)
                    && !matches!(
                        fact.support,
                        SupportKind::Air | SupportKind::Water | SupportKind::Lava
                    )
            })
    };
    if placed.traits.contains(T::DOOR) {
        // A door goes on the top face, unless the clicked block is itself replaced.
        let upper = placement_cell(foot, 1);
        return (face == 1 || foot == clicked)
            && build_height.contains(&upper[1])
            && replaceable(upper)
            && supported(foot);
    }
    let Some(quadrant) = yaw_quadrant(yaw) else {
        return false;
    };
    let [x, z] = [[0, 1], [-1, 0], [0, -1], [1, 0]][quadrant];
    let head = [foot[0] + x, foot[1], foot[2] + z];
    replaceable(head)
        && supported(foot)
        && supported(head)
        && !(-BED_COLUMN_REACH..=BED_COLUMN_REACH)
            .map(|offset| foot[1] + offset)
            .filter(|y| build_height.contains(y))
            .any(|y| {
                read([foot[0], y, foot[2]]).is_some_and(|fact| fact.family == ModelFamily::Bed)
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::block_fact;

    fn surroundings(clicked: &str, neighbor: &str) -> UseSurroundings {
        UseSurroundings {
            clicked: block_fact(clicked),
            clicked_canonical_state: None,
            held: BlockFact::default(),
            neighbor: block_fact(neighbor),
            player_box: ([100.0; 3], [101.0; 3]),
            actor_boxes: Vec::new(),
            sneaking: false,
            placed_boxes: None,
        }
    }

    /// Stone at and below y=0, with the named cells overriding the air above it.
    fn world(cells: &[([i32; 3], &str)], cell: [i32; 3]) -> Option<BlockFact> {
        let named = cells.iter().find(|(position, _)| *position == cell);
        Some(block_fact(named.map_or(
            if cell[1] <= 0 {
                "minecraft:stone"
            } else {
                "minecraft:air"
            },
            |(_, name)| *name,
        )))
    }

    const BUILD_HEIGHT: std::ops::Range<i32> = -64..320;

    fn fits(block: &str, yaw: f32, cells: &[([i32; 3], &str)]) -> bool {
        paired_item_fits(
            block_fact(block),
            [0, 0, 0],
            1,
            yaw,
            &surroundings("minecraft:stone", "minecraft:air"),
            BUILD_HEIGHT,
            |cell| world(cells, cell),
        )
    }

    #[test]
    fn a_door_needs_the_cell_above_it_clear() {
        assert!(fits("minecraft:wooden_door", 0.0, &[]));
        assert!(!fits(
            "minecraft:wooden_door",
            0.0,
            &[([0, 2, 0], "minecraft:stone")]
        ));
    }

    #[test]
    fn a_door_needs_both_halves_inside_the_build_height() {
        assert!(fits("minecraft:wooden_door", 0.0, &[]));
        let top = |build_height| {
            paired_item_fits(
                block_fact("minecraft:wooden_door"),
                [0, 0, 0],
                1,
                0.0,
                &surroundings("minecraft:stone", "minecraft:air"),
                build_height,
                |cell| world(&[], cell),
            )
        };
        assert!(top(-64..3));
        assert!(!top(-64..2));
        assert!(!top(-64..1));
    }

    #[test]
    fn a_door_on_a_side_face_needs_a_replaceable_clicked_block() {
        let side = |clicked| {
            paired_item_fits(
                block_fact("minecraft:wooden_door"),
                [0, 1, 0],
                5,
                0.0,
                &surroundings(clicked, "minecraft:air"),
                BUILD_HEIGHT,
                |cell| world(&[], cell),
            )
        };
        assert!(!side("minecraft:stone"));
        assert!(side("minecraft:short_grass"));
    }

    #[test]
    fn a_bed_head_follows_the_yaw_quadrant_and_needs_room_and_support() {
        for (yaw, head) in [
            (0.0, [0, 1, 1]),
            (90.0, [-1, 1, 0]),
            (180.0, [0, 1, -1]),
            (-90.0, [1, 1, 0]),
        ] {
            assert!(fits("minecraft:bed", yaw, &[]));
            assert!(!fits("minecraft:bed", yaw, &[(head, "minecraft:stone")]));
            let below = [head[0], 0, head[2]];
            assert!(!fits("minecraft:bed", yaw, &[(below, "minecraft:air")]));
        }
    }

    #[test]
    fn a_bed_refuses_another_bed_in_its_foot_column() {
        for y in [-2, 4] {
            assert!(!fits("minecraft:bed", 0.0, &[([0, y, 0], "minecraft:bed")]));
        }
        assert!(fits("minecraft:bed", 0.0, &[([0, 5, 0], "minecraft:bed")]));
    }

    #[test]
    fn a_bed_reads_no_cell_outside_the_build_height() {
        let outside = std::cell::Cell::new(false);
        let build_height = 0..2;
        assert!(paired_item_fits(
            block_fact("minecraft:bed"),
            [0, 0, 0],
            1,
            0.0,
            &surroundings("minecraft:stone", "minecraft:air"),
            build_height.clone(),
            |cell| {
                outside.set(outside.get() || !build_height.contains(&cell[1]));
                world(&[], cell)
            },
        ));
        assert!(!outside.get());
    }

    #[test]
    fn a_blocked_destination_fits_nothing() {
        assert!(!paired_item_fits(
            block_fact("minecraft:bed"),
            [0, 0, 0],
            1,
            0.0,
            &surroundings("minecraft:stone", "minecraft:stone"),
            BUILD_HEIGHT,
            |cell| world(&[], cell),
        ));
    }
}
