//! Chorus plants grow arms toward their neighbours and short stubs elsewhere; flowers are three
//! crossed bars, drawn with withered art at their final age.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::vanilla_cuboid_quads;
use assets::chorus;
use assets::facts::pack::PackBlock as B;
use {super::super::*, assets::BlockFace};

/// Inner and outer faces of the plant's central cube.
const CORE: [i16; 2] = [64, 192];
/// Cross section of a thin stub.
const THIN: [i16; 2] = [80, 176];
/// The outer end of a connected arm, a short stub and a thin stub, measured from the core.
const REACH_CONNECTED: i16 = 64;
const REACH_SHORT: i16 = 16;
const REACH_THIN: i16 = 32;

pub(in crate::compiler) fn is_plant(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::ChorusPlant.identifier()
}

/// Marks the one block a plant above treats as ground to root into.
pub(in crate::compiler) fn apply_root_flag(record: &RegistryRecord, visual: &mut BlockVisual) {
    visual.flags.set(
        BlockFlags::CHORUS_ROOT,
        record.name.as_ref() == B::EndStone.identifier(),
    );
}

/// Resolves the plant's one sprite, keeping its cut-out pixels regardless of the fallback table.
pub(in crate::compiler) fn plant_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let key = resolve_texture_key(&pack.blocks, record, face).key?;
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, 0)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
        },
        key,
    ))
}

/// One of the six directions an arm grows in.
#[derive(Clone, Copy)]
struct Direction {
    connection: u32,
    axis: usize,
    positive: bool,
    /// Position classes that draw a full-width stub and the one that draws a thin one.
    short_classes: [u32; 2],
    thin_class: u32,
}

const DIRECTIONS: [Direction; 6] = [
    Direction {
        connection: chorus::NORTH,
        axis: 2,
        positive: false,
        short_classes: [1, 3],
        thin_class: 2,
    },
    Direction {
        connection: chorus::EAST,
        axis: 0,
        positive: true,
        short_classes: [0, 2],
        thin_class: 1,
    },
    Direction {
        connection: chorus::SOUTH,
        axis: 2,
        positive: true,
        short_classes: [1, 3],
        thin_class: 0,
    },
    Direction {
        connection: chorus::WEST,
        axis: 0,
        positive: false,
        short_classes: [0, 2],
        thin_class: 3,
    },
    Direction {
        connection: chorus::UP,
        axis: 1,
        positive: true,
        short_classes: [1, 2],
        thin_class: 3,
    },
    Direction {
        connection: chorus::DOWN,
        axis: 1,
        positive: false,
        short_classes: [0, 2],
        thin_class: 1,
    },
];

/// The block face looking along `direction`, away from the core.
fn outward_face(direction: Direction) -> BlockFace {
    match (direction.axis, direction.positive) {
        (0, false) => BlockFace::West,
        (0, true) => BlockFace::East,
        (1, false) => BlockFace::Down,
        (1, true) => BlockFace::Up,
        (_, false) => BlockFace::North,
        (_, true) => BlockFace::South,
    }
}

fn opposite(face: BlockFace) -> BlockFace {
    match face {
        BlockFace::West => BlockFace::East,
        BlockFace::East => BlockFace::West,
        BlockFace::Down => BlockFace::Up,
        BlockFace::Up => BlockFace::Down,
        BlockFace::North => BlockFace::South,
        BlockFace::South => BlockFace::North,
    }
}

/// The piece growing out of the core for a connection mask and position class: its cross
/// section and how far it reaches. Nothing grows for one class in each direction.
fn piece(direction: Direction, connections: u32, class: u32) -> Option<([i16; 2], i16)> {
    if connections & direction.connection != 0 {
        Some((CORE, REACH_CONNECTED))
    } else if direction.short_classes.contains(&class) {
        Some((CORE, REACH_SHORT))
    } else if direction.thin_class == class {
        Some((THIN, REACH_THIN))
    } else {
        None
    }
}

/// Faces of one cuboid except the one touching the core, which nothing can see.
fn arm_quads(material: u32, direction: Direction, section: [i16; 2], reach: i16) -> Vec<ModelQuad> {
    let (near, far) = if direction.positive {
        (CORE[1], CORE[1] + reach)
    } else {
        (CORE[0] - reach, CORE[0])
    };
    let mut min = [section[0]; 3];
    let mut max = [section[1]; 3];
    min[direction.axis] = near;
    max[direction.axis] = far;
    let hidden = opposite(outward_face(direction));
    BlockFace::ALL
        .into_iter()
        .zip(vanilla_cuboid_quads([material; 6], min, max))
        .filter(|&(face, _)| face != hidden)
        .map(|(_, quad)| quad)
        .collect()
}

/// A plant's visible quads: arms and stubs, plus each core face no full-width piece covers.
pub(in crate::compiler) fn plant_quads(
    material: u32,
    connections: u32,
    class: u32,
) -> Vec<ModelQuad> {
    let core = vanilla_cuboid_quads([material; 6], [CORE[0]; 3], [CORE[1]; 3]);
    let mut quads = Vec::new();
    for direction in DIRECTIONS {
        let grown = piece(direction, connections, class);
        if let Some((section, reach)) = grown {
            quads.extend(arm_quads(material, direction, section, reach));
        }
        if !grown.is_some_and(|(section, _)| section == CORE) {
            let face = outward_face(direction) as usize;
            quads.push(core[face]);
        }
    }
    quads
}

/// The age at which a flower can no longer grow and shows its withered art.
const WITHERED_AGE: u32 = 5;

pub(in crate::compiler) fn is_flower(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::ChorusFlower.identifier()
}

/// The terrain array entry a flower's age selects, for flowers only.
pub(in crate::compiler) fn flower_variant(record: &RegistryRecord) -> Option<usize> {
    if !is_flower(record) {
        return None;
    }
    let age = canonical_state_u32(
        &record.canonical_state,
        assets::facts::BlockProperty::Age.name(),
    )?;
    Some(usize::from(age == WITHERED_AGE))
}

/// Three full-length bars along each axis, each with one cross section.
fn flower_quads(materials: [u32; 6]) -> Vec<ModelQuad> {
    const LOW: i16 = 32;
    const HIGH: i16 = 224;
    (0..3)
        .flat_map(|axis| {
            let mut min = [LOW; 3];
            let mut max = [HIGH; 3];
            min[axis] = 0;
            max[axis] = 256;
            vanilla_cuboid_quads(materials, min, max)
        })
        .collect()
}

/// Compiles flowers, leaving packs without their art to the provisional shape.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 6], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_flower(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let template = if let Some(&template) = templates.get(&materials) {
        template
    } else {
        let template = push_model_template(
            flower_quads(materials),
            assets::MODEL_TEMPLATE_FLAG_CHORUS,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(materials, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Compiles a plant's whole template bank once per art; packs without the art stay diagnostic.
pub(in crate::compiler) fn compile_plant_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<u32, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_plant(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(material) = inputs.material(record, BlockFace::Up) {
        let template = if let Some(&template) = templates.get(&material) {
            template
        } else {
            let first = storage.templates.len() as u32;
            for offset in 0..chorus::TEMPLATE_COUNT {
                let quads = plant_quads(material, offset & 63, offset >> 6);
                push_model_template(
                    quads,
                    assets::MODEL_TEMPLATE_FLAG_CHORUS_PLANT,
                    storage.templates,
                    storage.quads,
                )?;
            }
            templates.insert(material, first);
            first
        };
        set_model_visual(&mut visual, [material; 6], template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The extent along each axis of a quad set, as `[min, max]` per axis.
    fn bounds(quads: &[ModelQuad]) -> [[i16; 2]; 3] {
        std::array::from_fn(|axis| {
            let values = quads
                .iter()
                .flat_map(|quad| quad.positions.map(|p| p[axis]));
            [values.clone().min().unwrap(), values.max().unwrap()]
        })
    }

    /// Quads of the piece growing out in one direction, excluding the core.
    fn beyond(quads: &[ModelQuad], axis: usize, positive: bool) -> Vec<ModelQuad> {
        quads
            .iter()
            .filter(|quad| {
                quad.positions.iter().any(|p| {
                    if positive {
                        p[axis] > CORE[1]
                    } else {
                        p[axis] < CORE[0]
                    }
                })
            })
            .copied()
            .collect()
    }

    #[test]
    fn connected_arms_reach_the_block_edge_in_every_class() {
        let every = chorus::NORTH | chorus::EAST | chorus::SOUTH | chorus::WEST;
        for class in 0..chorus::POSITION_CLASSES {
            let quads = plant_quads(9, every | chorus::UP | chorus::DOWN, class);
            assert_eq!(bounds(&quads), [[0, 256]; 3], "class {class}");
        }
    }

    #[test]
    fn unconnected_stubs_vary_with_position_class() {
        let north = |class| beyond(&plant_quads(9, 0, class), 2, false);
        assert!(north(0).is_empty());
        assert_eq!(bounds(&north(1))[2], [48, 64]);
        assert_eq!(bounds(&north(2))[2], [32, 64]);
        assert_eq!(bounds(&north(2))[0], THIN);
        assert_eq!(bounds(&north(3))[2], [48, 64]);

        let up = |class| beyond(&plant_quads(9, 0, class), 1, true);
        assert!(up(0).is_empty());
        assert_eq!(bounds(&up(1))[1], [192, 208]);
        assert_eq!(bounds(&up(2))[1], [192, 208]);
        assert_eq!(bounds(&up(3))[1], [192, 224]);
        assert_eq!(bounds(&up(3))[0], THIN);

        let down = |class| beyond(&plant_quads(9, 0, class), 1, false);
        assert_eq!(bounds(&down(0))[1], [48, 64]);
        assert_eq!(bounds(&down(1))[1], [32, 64]);
        assert!(down(3).is_empty());
    }

    #[test]
    fn every_template_fits_the_quad_limit_and_shows_the_core_where_nothing_covers_it() {
        for offset in 0..chorus::TEMPLATE_COUNT {
            let quads = plant_quads(9, offset & 63, offset >> 6);
            assert!(quads.len() <= assets::MAX_MODEL_TEMPLATE_QUADS, "{offset}");
        }
        // Class 0 grows nothing north: the core's own north face closes the gap.
        let quads = plant_quads(9, 0, 0);
        assert!(quads.iter().any(|quad| {
            quad.positions
                .iter()
                .all(|p| p[2] == CORE[0] && CORE.contains(&p[0]) && CORE.contains(&p[1]))
        }));
        // With every side connected no core face remains.
        let quads = plant_quads(9, 63, 0);
        assert_eq!(quads.len(), 30);
    }

    #[test]
    fn flowers_are_three_full_length_bars() {
        let quads = flower_quads([9; 6]);
        assert_eq!(quads.len(), 18);
        for (axis, bar) in quads.chunks(6).enumerate() {
            let extent = bounds(bar);
            for (other, range) in extent.iter().enumerate() {
                assert_eq!(*range, if other == axis { [0, 256] } else { [32, 224] });
            }
        }
    }
}
