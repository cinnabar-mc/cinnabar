use render_model::{BedModel, BlockEntityKind, BlockEntityLight, bed_color};
use world::{BlockEntityKey, ChunkStore};

use super::{
    super::describe::bed, BlockEntityRuntime, PhysicsCollisionRegistries, block_info,
    portals::PortalCell,
};

/// Whether `runtime_id` is a bed block, which a bed block entity draws.
pub(super) fn is_bed(
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    runtime_id: u32,
) -> bool {
    assets::facts::BlockEntityFacts::pinned()
        .for_registry(runtime_id, mode, collisions.breg_sha256())
        .is_some_and(|backing| backing.entity.renderer == assets::facts::BlockEntityRenderer::Bed)
}

/// The pillow half of a bed block in the default colour, drawn from the palette until a block
/// entity colours it. The foot half draws nothing.
pub(super) fn resting(
    runtime: &mut BlockEntityRuntime,
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    runtime_id: u32,
) -> Option<BlockEntityKind> {
    let info = block_info(runtime, collisions, mode, runtime_id)?;
    if info.backing?.entity.renderer != assets::facts::BlockEntityRenderer::Bed {
        return None;
    }
    bed(&info.state, None)
        .filter(|model| model.head)
        .map(BlockEntityKind::Bed)
}

/// The dye a bed block entity names at `block`, if there is one.
pub(super) fn entity_dye(store: &ChunkStore, dimension: i32, [x, y, z]: [i32; 3]) -> Option<i64> {
    store
        .block_entity(BlockEntityKey::new(dimension, x, y, z))?
        .parsed()?
        .integer("color")
}

/// Settles palette-drawn beds against their block entities, read through `dye_at`. A head whose
/// own entity names a drawable dye is left to the entity path; any other head takes the dye of
/// the foot's entity, else keeps the default.
pub(super) fn settle(cells: &mut Vec<PortalCell>, dye_at: impl Fn([i32; 3]) -> Option<i64>) {
    cells.retain_mut(|(block, kind)| {
        let BlockEntityKind::Bed(model) = kind else {
            return true;
        };
        if dye_at(*block).is_some_and(|dye| bed_color(dye).is_some()) {
            return false;
        }
        let offset = model.other_half_offset();
        let partner = std::array::from_fn(|axis| block[axis] + offset[axis]);
        if let Some(color) = dye_at(partner).and_then(bed_color) {
            *model = BedModel { color, ..*model };
        }
        true
    });
}

/// Uses the brightest block and sky light from the bed's two halves.
pub(super) fn light(
    model: BedModel,
    position: [i32; 3],
    mut sample: impl FnMut([f32; 3]) -> (u8, u8),
) -> BlockEntityLight {
    let center = position.map(|value| value as f32 + 0.5);
    let offset = model.other_half_offset();
    let other = std::array::from_fn(|axis| center[axis] + offset[axis] as f32);
    let (block, sky) = sample(center);
    let (other_block, other_sky) = sample(other);
    BlockEntityLight::Actor {
        block: block.max(other_block),
        sky: sky.max(other_sky),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(direction: u8) -> PortalCell {
        (
            [4, 64, 4],
            BlockEntityKind::Bed(BedModel {
                color: "red",
                head: true,
                direction,
            }),
        )
    }

    /// Regression: a bed block with no block entity was never drawn.
    #[test]
    fn a_bed_without_a_block_entity_keeps_the_default_colour() {
        let mut cells = vec![head(0)];
        settle(&mut cells, |_| None);
        assert_eq!(cells, [head(0)]);
    }

    #[test]
    fn a_bed_whose_entity_names_a_dye_is_drawn_by_the_entity() {
        let mut cells = vec![head(0)];
        settle(&mut cells, |block| (block == [4, 64, 4]).then_some(11));
        assert!(cells.is_empty());
    }

    #[test]
    fn a_bed_takes_the_colour_of_its_foot_entity_and_ignores_unknown_dyes() {
        // Direction 0 points the head south, so the foot lies at -z.
        let mut cells = vec![head(0)];
        settle(&mut cells, |block| (block == [4, 64, 3]).then_some(11));
        let BlockEntityKind::Bed(model) = &cells[0].1 else {
            panic!("bed expected");
        };
        assert_eq!(model.color, "blue");
        let mut cells = vec![head(0)];
        settle(&mut cells, |block| (block == [4, 64, 4]).then_some(99));
        assert_eq!(cells, [head(0)]);
    }

    #[test]
    fn bed_lighting_keeps_the_brightest_block_and_sky_channel_from_both_halves() {
        for (direction, offset) in [
            (0, [0.0, 0.0, -1.0]),
            (1, [1.0, 0.0, 0.0]),
            (2, [0.0, 0.0, 1.0]),
            (3, [-1.0, 0.0, 0.0]),
        ] {
            let head = [10.5, 64.5, -9.5];
            let foot = std::array::from_fn(|axis| head[axis] + offset[axis]);
            let mut sampled = Vec::new();
            let result = light(
                BedModel {
                    color: "red",
                    head: true,
                    direction,
                },
                [10, 64, -10],
                |position| {
                    sampled.push(position);
                    if position == head { (2, 15) } else { (12, 4) }
                },
            );
            assert_eq!(sampled, [head, foot]);
            assert_eq!(result, BlockEntityLight::Actor { block: 12, sky: 15 });
        }
    }
}
