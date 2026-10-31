//! Planting with a `minecraft:seed` item, which places a crop without being a block item.

use assets::facts::pack::PackBlock;

/// The cell a seed item plants into when used on `clicked`'s `face`, or `None` when the
/// use fails or depends on facts the client does not resolve.
///
/// The face must be the declared planting face, the clicked block one of the declared
/// blocks, and the cell beyond that face air. A rule that declares no blocks and no
/// solid-surface option plants on farmland, the native default. Planting on an undeclared
/// block through `plant_at_any_solid_surface` needs native face-support facts and is left to
/// the server.
pub fn seed_planting_cell(
    seed: &protocol::ItemSeed,
    clicked: [i32; 3],
    face: u8,
    clicked_block: &str,
    is_air: impl FnOnce([i32; 3]) -> bool,
) -> Option<[i32; 3]> {
    let accepted = if seed.plant_at.is_empty() && !seed.plant_at_any_solid_surface {
        clicked_block == PackBlock::Farmland.identifier()
    } else {
        seed.plant_at
            .iter()
            .any(|declared| same_block(declared, clicked_block))
    };
    if face != seed.plant_at_face || !accepted {
        return None;
    }
    let cell = super::placement_cell(clicked, face);
    is_air(cell).then_some(cell)
}

/// Compares a declared block name with a registry identifier; bare names are vanilla blocks.
fn same_block(declared: &str, identifier: &str) -> bool {
    if declared.contains(':') {
        declared == identifier
    } else {
        identifier.strip_prefix("minecraft:") == Some(declared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn seed(plant_at: &[&str], face: u8) -> protocol::ItemSeed {
        protocol::ItemSeed {
            crop_result: "minecraft:wheat".into(),
            plant_at: plant_at.iter().map(|name| Arc::from(*name)).collect(),
            plant_at_any_solid_surface: false,
            plant_at_face: face,
        }
    }

    #[test]
    fn wheat_seeds_plant_above_the_top_of_farmland() {
        let wheat = seed(&["minecraft:farmland"], 1);
        assert_eq!(
            seed_planting_cell(&wheat, [4, 63, -2], 1, "minecraft:farmland", |cell| {
                cell == [4, 64, -2]
            }),
            Some([4, 64, -2])
        );
    }

    #[test]
    fn a_rule_without_targets_plants_only_on_farmland() {
        let pitcher = seed(&[], 1);
        let air = |_| true;
        assert!(seed_planting_cell(&pitcher, [0, 0, 0], 1, "minecraft:farmland", air).is_some());
        assert!(seed_planting_cell(&pitcher, [0, 0, 0], 1, "minecraft:dirt", air).is_none());
    }

    #[test]
    fn other_faces_blocks_or_occupied_cells_do_not_plant() {
        let wheat = seed(&["minecraft:farmland"], 1);
        let air = |_| true;
        assert_eq!(
            seed_planting_cell(&wheat, [0, 0, 0], 2, "minecraft:farmland", air),
            None
        );
        assert_eq!(
            seed_planting_cell(&wheat, [0, 0, 0], 1, "minecraft:dirt", air),
            None
        );
        assert_eq!(
            seed_planting_cell(&wheat, [0, 0, 0], 1, "minecraft:farmland", |_| false),
            None
        );
    }

    #[test]
    fn bare_declared_names_match_only_vanilla_blocks() {
        let wart = seed(&["soul_sand"], 1);
        let air = |_| true;
        assert!(seed_planting_cell(&wart, [0, 0, 0], 1, "minecraft:soul_sand", air).is_some());
        assert!(seed_planting_cell(&wart, [0, 0, 0], 1, "custom:soul_sand", air).is_none());
    }

    #[test]
    fn hanging_crops_plant_below_the_clicked_bottom_face() {
        let berries = seed(&["cave_vines"], 0);
        assert_eq!(
            seed_planting_cell(&berries, [1, 70, 1], 0, "minecraft:cave_vines", |_| true),
            Some([1, 69, 1])
        );
    }

    #[test]
    fn solid_surface_planting_on_undeclared_blocks_stays_with_the_server() {
        let mut berries = seed(&[], 0);
        berries.plant_at_any_solid_surface = true;
        assert_eq!(
            seed_planting_cell(&berries, [0, 0, 0], 0, "minecraft:stone", |_| true),
            None
        );
    }
}
