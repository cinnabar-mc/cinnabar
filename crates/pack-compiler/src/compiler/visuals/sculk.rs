//! Sculk sensors grow crossed tendrils from a half-height body; shriekers are an open bowl.
//!
//! The tendrils and the shrieker's inner floor use carried art, registered as a material of its
//! own beside the six body faces.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::vanilla_cuboid_quads;
use assets::facts::pack::PackBlock as B;
use {super::super::*, assets::BlockFace};

const BODY: ([i16; 3], [i16; 3]) = ([0; 3], [256, 128, 256]);
const TEETH: ([i16; 3], [i16; 3]) = ([16, 128, 16], [240; 3]);
/// How far a wall's inner surface sits from its outer surface.
const WALL_OFFSET: i16 = 224;
const TENDRIL_NEAR: i16 = 48;
const TENDRIL_FAR: i16 = 208;
const TENDRIL_HALF: i16 = 92;
const TENDRIL_BOTTOM: i16 = 128;
const TENDRIL_TOP: i16 = 384;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    Sensor,
    Shrieker,
}

fn kind(record: &RegistryRecord) -> Option<Kind> {
    let name = record.name.as_ref();
    if name == B::SculkSensor.identifier() {
        Some(Kind::Sensor)
    } else if name == B::SculkShrieker.identifier() {
        Some(Kind::Shrieker)
    } else {
        None
    }
}

/// Whether the record is a sculk sensor or shrieker, which carry attachment art.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    kind(record).is_some()
}

fn state_value(record: &RegistryRecord, property: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(&record.canonical_state)
        .ok()?
        .get(property)?
        .get("value")?
        .as_u64()
}

/// Selects the tendril art a sensor phase carries, or the inner floor a shrieker's summoning
/// state carries.
pub(in crate::compiler) fn attachment_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<(Descriptor, Box<str>)> {
    let (face, variant) = match kind(record)? {
        Kind::Sensor
            if state_value(
                record,
                assets::facts::BlockProperty::SculkSensorPhase.name(),
            )? == 0 =>
        {
            (BlockFace::Down, 0)
        }
        Kind::Sensor => (BlockFace::East, 0),
        Kind::Shrieker => (
            BlockFace::Up,
            usize::from(state_value(record, assets::facts::BlockProperty::CanSummon.name())? != 0),
        ),
    };
    let key: Box<str> =
        pack_compile_input::pack::resolve_carried_face_key(&pack.blocks, record, face)?.into();
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, variant)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: Some((state_variant, state_variant)),
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
        },
        key,
    ))
}

/// One cuboid face, cropped by its own bounds and then moved, optionally culled by the neighbour
/// on that side.
fn surface(
    face: BlockFace,
    (min, max): ([i16; 3], [i16; 3]),
    offset: [i16; 3],
    material: u32,
    culled: bool,
) -> ModelQuad {
    let mut quad = vanilla_cuboid_quads([material; 6], min, max)[face as usize];
    for position in &mut quad.positions {
        for axis in 0..3 {
            position[axis] += offset[axis];
        }
    }
    if culled {
        quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
    }
    quad
}

/// A half-height body open to the sky, with four two-sided tendril planes standing on it.
fn sensor_quads(faces: [u32; 6], tendril: u32) -> Vec<ModelQuad> {
    let mut quads: Vec<_> = BlockFace::ALL
        .into_iter()
        .map(|face| {
            surface(
                face,
                BODY,
                [0; 3],
                faces[face as usize],
                face != BlockFace::Up,
            )
        })
        .collect();
    for ([x, z], slope) in [
        ([TENDRIL_NEAR, TENDRIL_FAR], 1),
        ([TENDRIL_FAR, TENDRIL_FAR], -1),
        ([TENDRIL_NEAR, TENDRIL_NEAR], -1),
        ([TENDRIL_FAR, TENDRIL_NEAR], 1),
    ] {
        let (ax, az) = (x - TENDRIL_HALF, z - slope * TENDRIL_HALF);
        let (bx, bz) = (x + TENDRIL_HALF, z + slope * TENDRIL_HALF);
        quads.push(ModelQuad {
            positions: [
                [ax, TENDRIL_TOP, az],
                [ax, TENDRIL_BOTTOM, az],
                [bx, TENDRIL_BOTTOM, bz],
                [bx, TENDRIL_TOP, bz],
            ],
            uvs: [[0, 0], [0, 4096], [4096, 4096], [4096, 0]],
            material: tendril,
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        });
    }
    quads
}

/// Outer walls culled by their neighbour, uncullable inner walls facing the other way, an inner
/// floor, and a rim drawn on both its faces.
fn shrieker_quads(faces: [u32; 6], floor: u32) -> Vec<ModelQuad> {
    use BlockFace::{Down, East, North, South, Up, West};
    let top = faces[Up as usize];
    let mut quads = vec![surface(Down, BODY, [0; 3], faces[Down as usize], true)];
    for (outside, inside, offset) in [
        (North, South, [0, 0, -WALL_OFFSET]),
        (East, West, [WALL_OFFSET, 0, 0]),
        (South, North, [0, 0, WALL_OFFSET]),
        (West, East, [-WALL_OFFSET, 0, 0]),
    ] {
        for bounds in [BODY, TEETH] {
            quads.push(surface(
                outside,
                bounds,
                [0; 3],
                faces[outside as usize],
                true,
            ));
            quads.push(surface(
                inside,
                bounds,
                offset,
                faces[inside as usize],
                false,
            ));
        }
    }
    quads.push(surface(Up, BODY, [0; 3], floor, false));
    quads.push(surface(Up, TEETH, [0; 3], top, true));
    quads.push(surface(Down, BODY, [0, TEETH.1[1], 0], top, false));
    quads
}

/// Compiles both blocks from their body and attachment art, leaving packs without that art to
/// the provisional shape.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(bool, [u32; 6], u32), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    let Some(kind) = kind(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let Some(attachment) = attachment_descriptor(inputs.pack, record)
        .and_then(|(descriptor, _)| inputs.material_by_descriptor.get(&descriptor).copied())
    else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let key = (kind == Kind::Shrieker, materials, attachment);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = match kind {
            Kind::Sensor => sensor_quads(materials, attachment),
            Kind::Shrieker => shrieker_quads(materials, attachment),
        };
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}
