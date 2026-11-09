use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::vanilla_cuboid_quads;
use super::torches::{TorchKind, TorchPlacement, torch_quads};
use assets::facts::pack::PackBlock as B;
use assets::facts::{BlockProperty, BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace, assets::MODEL_QUAD_FLAG_FACE_MASK};

/// Plate height in 1/256 block; the torches stop at it.
const PLATE_TOP: i16 = 32;
const PLATE_FLOOR: f32 = 0.125;
/// A plate's rear torch's and output torch's distance from the block's centre.
const OUTPUT_TORCH: f32 = 0.3125;
const REAR_TORCH: f32 = 0.25;
const REAR_TORCH_SPREAD: f32 = 0.1875;
/// How far a repeater's delay torch sits behind the centre for each delay setting.
const DELAY_OFFSETS: [f32; 4] = [-0.0625, 0.0625, 0.1875, 0.3125];
/// Heights of a torch's origin below the block floor; the plate hides everything under it.
const STANDARD_DROP: f32 = -0.1875;
const SUBTRACT_DROP: f32 = -0.3125;
const COMPARE_DROP: f32 = -0.375;

/// Art of the two torch states every diode can show.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::compiler) struct TorchMaterials {
    pub(in crate::compiler) lit: Option<u32>,
    pub(in crate::compiler) unlit: Option<u32>,
}

impl TorchMaterials {
    /// Reads each torch state's own art so a pack that repaints it keeps its diodes in step.
    pub(in crate::compiler) fn find(records: &[RegistryRecord], inputs: &RuleInputs<'_>) -> Self {
        let material = |block: B| {
            records
                .iter()
                .find(|record| record.name.as_ref() == block.identifier())
                .and_then(|record| inputs.material(record, BlockFace::Up))
        };
        Self {
            lit: material(B::RedstoneTorch),
            unlit: material(B::UnlitRedstoneTorch),
        }
    }

    fn pick(self, lit: bool) -> Option<u32> {
        if lit { self.lit } else { self.unlit }
    }
}

/// Registry states resolved to where their torches stand.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum DiodePose {
    Repeater {
        direction: u8,
        delay: u8,
        powered: bool,
    },
    Comparator {
        direction: u8,
        subtract: bool,
        powered: bool,
    },
}

/// Reads a diode's state, or `None` when it is malformed and falls through to its bare plate.
fn pose(record: &RegistryRecord) -> Option<DiodePose> {
    if block_render_fact(&record.name).kind != R::RedstoneBase {
        return None;
    }
    let direction = match canonical_state_str(
        &record.canonical_state,
        BlockProperty::MinecraftCardinalDirection.name(),
    )?
    .as_ref()
    {
        "south" => 0,
        "west" => 1,
        "north" => 2,
        "east" => 3,
        _ => return None,
    };
    let name = record.name.as_ref();
    let powered = name.starts_with("minecraft:powered_");
    if let Some(delay) =
        canonical_state_u32(&record.canonical_state, BlockProperty::RepeaterDelay.name())
    {
        return (delay < DELAY_OFFSETS.len() as u32).then_some(DiodePose::Repeater {
            direction,
            delay: delay as u8,
            powered,
        });
    }
    let subtract = canonical_state_u32(
        &record.canonical_state,
        BlockProperty::OutputSubtractBit.name(),
    )? != 0;
    Some(DiodePose::Comparator {
        direction,
        subtract,
        powered,
    })
}

/// The direction a diode's front (output side) points in world x and z, for a cardinal direction.
const fn front(direction: u8) -> [f32; 2] {
    match direction {
        0 => [0.0, -1.0],
        1 => [1.0, 0.0],
        2 => [0.0, 1.0],
        _ => [-1.0, 0.0],
    }
}

/// Torch feet, in order, as `[x, drop, z, lit]`.
fn torches(pose: DiodePose) -> Vec<([f32; 3], bool)> {
    match pose {
        DiodePose::Repeater {
            direction,
            delay,
            powered,
        } => {
            let [x, z] = front(direction);
            let behind = -DELAY_OFFSETS[usize::from(delay)];
            vec![
                ([x * behind, STANDARD_DROP, z * behind], powered),
                ([x * OUTPUT_TORCH, STANDARD_DROP, z * OUTPUT_TORCH], powered),
            ]
        }
        DiodePose::Comparator {
            direction,
            subtract,
            powered,
        } => {
            let [x, z] = front(direction);
            let [side_x, side_z] = [z, x];
            let rear = |side: f32| {
                [
                    -x * REAR_TORCH + side_x * side * REAR_TORCH_SPREAD,
                    STANDARD_DROP,
                    -z * REAR_TORCH + side_z * side * REAR_TORCH_SPREAD,
                ]
            };
            let drop = if subtract {
                SUBTRACT_DROP
            } else {
                COMPARE_DROP
            };
            vec![
                (rear(1.0), powered),
                (rear(-1.0), powered),
                ([x * OUTPUT_TORCH, drop, z * OUTPUT_TORCH], subtract),
            ]
        }
    }
}

/// The plate's top art turns to face the diode's direction.
fn plate_quads(materials: [u32; 6], direction: u8) -> Vec<ModelQuad> {
    let mut quads = vanilla_cuboid_quads(materials, [0; 3], [256, PLATE_TOP, 256]);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        if face == BlockFace::Down {
            quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
        }
        if face == BlockFace::Up {
            for uv in &mut quad.uvs {
                let [u, v] = *uv;
                *uv = match direction {
                    1 => [v, 4096 - u],
                    2 => [4096 - u, 4096 - v],
                    3 => [4096 - v, u],
                    _ => [u, v],
                };
            }
        }
    }
    quads.to_vec()
}

pub(in crate::compiler) type TemplateKey = ([u32; 6], DiodePose, [Option<u32>; 2]);

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    torch_materials: TorchMaterials,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    let Some(pose) = pose(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let key = (
        materials,
        pose,
        [torch_materials.lit, torch_materials.unlit],
    );
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let direction = match pose {
            DiodePose::Repeater { direction, .. } | DiodePose::Comparator { direction, .. } => {
                direction
            }
        };
        let mut quads = plate_quads(materials, direction);
        for ([x, drop, z], lit) in torches(pose) {
            if let Some(material) = torch_materials.pick(lit) {
                // Native lights the torches from the diode's own cell and only the plate from its
                // neighbours; a quad with no face takes that own-cell path.
                quads.extend(
                    torch_quads(
                        material,
                        TorchKind::Redstone,
                        TorchPlacement {
                            origin: [x, drop, z],
                            tilt: [0.0; 2],
                            floor: PLATE_FLOOR,
                        },
                    )
                    .into_iter()
                    .map(|quad| ModelQuad {
                        flags: quad.flags & !MODEL_QUAD_FLAG_FACE_MASK,
                        ..quad
                    }),
                );
            }
        }
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}
