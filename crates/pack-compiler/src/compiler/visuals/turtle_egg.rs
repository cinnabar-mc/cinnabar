//! Turtle eggs draw one to four separate cuboids, each stretched over its own patch of the art.
//!
//! Only an egg's underside touches its cell boundary, so only that face is culled.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::{UvRect, cuboid_quads, stretch_uvs};
use assets::facts::{BlockProperty, PropertyDomain, StateValue, pack::PackBlock as B};
use {super::super::*, assets::BlockFace};

/// One egg's bounds in sixteenths of a block, and the art patches its faces stretch over.
struct Egg {
    min: [i16; 3],
    max: [i16; 3],
    top_and_bottom: UvRect,
    sides: UvRect,
}

const fn sixteenths(value: u16) -> u16 {
    value * 256
}

const fn patch(u0: u16, u1: u16, v0: u16, v1: u16) -> UvRect {
    [
        sixteenths(u0),
        sixteenths(u1),
        sixteenths(v0),
        sixteenths(v1),
    ]
}

/// Eggs in drawing order: a state with N eggs draws the first N.
const EGGS: [Egg; 4] = [
    Egg {
        min: [5, 0, 4],
        max: [10, 7, 9],
        top_and_bottom: patch(0, 4, 0, 4),
        sides: patch(1, 5, 4, 11),
    },
    Egg {
        min: [1, 0, 7],
        max: [5, 5, 11],
        top_and_bottom: patch(6, 10, 7, 11),
        sides: patch(10, 14, 10, 15),
    },
    Egg {
        min: [11, 0, 7],
        max: [14, 4, 10],
        top_and_bottom: patch(5, 8, 0, 3),
        sides: patch(8, 11, 3, 7),
    },
    Egg {
        min: [7, 0, 10],
        max: [10, 3, 13],
        top_and_bottom: patch(0, 4, 11, 15),
        sides: patch(4, 8, 11, 15),
    },
];

fn egg_quads(material: u32, count: usize) -> Vec<ModelQuad> {
    let mut quads = Vec::with_capacity(count * BlockFace::ALL.len());
    for egg in &EGGS[..count] {
        let bounds = |corner: [i16; 3]| corner.map(|axis| axis * 16);
        let faces = cuboid_quads([material; 6], bounds(egg.min), bounds(egg.max));
        for (face, quad) in BlockFace::ALL.into_iter().zip(faces) {
            let rect = match face {
                BlockFace::Up | BlockFace::Down => egg.top_and_bottom,
                _ => egg.sides,
            };
            let mut quad = stretch_uvs(quad, rect, false);
            if face == BlockFace::Down {
                quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
            }
            quads.push(quad);
        }
    }
    quads
}

/// The number of eggs a state draws, from its property's domain order.
fn egg_count(record: &RegistryRecord) -> Option<usize> {
    let StateValue::String(value) = record.property(BlockProperty::TurtleEggCount)? else {
        return None;
    };
    let PropertyDomain::String(domain) = BlockProperty::TurtleEggCount.domain() else {
        return None;
    };
    Some(domain.iter().position(|name| *name == value.as_ref())? + 1)
}

/// Whether the record is a turtle egg, whose exact rule replaces its provisional envelope.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::TurtleEgg.identifier()
}

/// Compiles one egg state from the art its cracking selects.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, usize), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let (Some(materials), Some(count)) = (inputs.materials(record), egg_count(record)) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let key = (materials[BlockFace::Up as usize], count);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = egg_quads(key.0, count);
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_draws_the_first_eggs_of_the_fixed_arrangement() {
        for count in 1..=EGGS.len() {
            let quads = egg_quads(1, count);
            assert_eq!(quads.len(), count * 6);
            let first_top = quads
                .iter()
                .find(|quad| quad.positions.iter().all(|corner| corner[1] == 7 * 16))
                .unwrap();
            assert!(first_top.positions.iter().all(|corner| {
                (5 * 16..=10 * 16).contains(&corner[0]) && (4 * 16..=9 * 16).contains(&corner[2])
            }));
        }
    }

    #[test]
    fn only_an_eggs_underside_can_be_culled_and_it_samples_its_own_patch() {
        let quads = egg_quads(1, 1);
        for (face, quad) in BlockFace::ALL.into_iter().zip(&quads) {
            let culled = quad.flags & (MODEL_QUAD_FLAG_FACE_MASK << 4) != 0;
            assert_eq!(culled, face == BlockFace::Down, "{face:?}");
            let [u0, u1, v0, v1] = match face {
                BlockFace::Up | BlockFace::Down => patch(0, 4, 0, 4),
                _ => patch(1, 5, 4, 11),
            };
            assert!(quad.uvs.iter().all(|[u, v]| {
                (*u == u0 || *u == u1) && (*v == v0 || *v == v1)
            }));
        }
    }
}
