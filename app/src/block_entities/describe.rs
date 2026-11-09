//! Turns a block entity's id, backing block state and NBT into what the renderer draws.

mod item;
use item::held_item;

use assets::{
    banner::{MAX_BANNER_LAYERS, color_linear as banner_color, pattern_texture},
    block_entity_geometry::shulker_color_from_block_name,
    facts::{BlockEntityBacking, BlockEntityMount, BlockEntityRenderer, BlockProperty},
};
use std::sync::Arc;

use render_model::{
    BannerLayer, BannerModel, BannerMount, BedModel, BellAttachment, BlockEntityKind, ChestModel,
    ChestPair, ChestVariant, DEFAULT_BED_DYE, DecoratedPotModel, Facing,
    ITEM_FRAME_ROTATION_STEP_DEGREES, ItemFrameModel, Oxidation, ShulkerModel, SignMount,
    SkullKind, SkullModel, SkullMount, StatueModel, StatuePose, bed_color, sherd_pattern,
};
use world::NbtCompound;

use super::{sign_text::SignTextSpec, state::BlockState};

const DEFAULT_SIGN_COLOR: i32 = -0x1000000;

/// An item stack held by a block entity.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HeldItem {
    pub(super) identifier: Arc<str>,
    pub(super) metadata: u32,
    pub(super) shield_pattern: Option<Arc<render_api::ShieldPattern>>,
}

/// A retained plant block with all typed state selectors used to resolve its world identity.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlantBlock {
    pub(super) identifier: Arc<str>,
    pub(super) states: serde_json::Map<String, serde_json::Value>,
}

/// Reads only usable block-state tag types; malformed plants do not become inventory icons.
fn plant_block(plant: &NbtCompound) -> Option<PlantBlock> {
    let name = plant
        .string("name")
        .filter(|name| !name.is_empty() && *name != assets::facts::block::AIR_IDENTIFIER)?;
    let mut states = serde_json::Map::new();
    if let Some(compound) = plant.compound("states") {
        for (key, value) in compound.iter() {
            let typed = match value {
                world::NbtValue::Byte(value) => serde_json::json!({"type":"byte", "value":value}),
                world::NbtValue::Int(value) => serde_json::json!({"type":"int", "value":value}),
                world::NbtValue::String(value) => {
                    serde_json::json!({"type":"string", "value":value})
                }
                _ => return None,
            };
            states.insert(key.to_owned(), typed);
        }
    } else if plant.get("states").is_some() {
        return None;
    }
    Some(PlantBlock {
        identifier: Arc::from(name),
        states,
    })
}

/// A block entity's drawing plan before per-frame animation and text resolution.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Template {
    Static(BlockEntityKind),
    ItemFrame {
        model: ItemFrameModel,
        item: Option<HeldItem>,
        rotation_degrees: f32,
        /// The filled map's id when the framed item is a map.
        map_id: Option<i64>,
    },
    FlowerPot {
        plant: PlantBlock,
    },
    Campfire {
        yaw_degrees: f32,
        items: [Option<HeldItem>; 4],
    },
    Bell {
        attachment: BellAttachment,
        /// The side the last ring struck from, as the bell's record stores it.
        ring_direction: u8,
    },
    Conduit {
        active: bool,
        hunting: bool,
    },
    Beacon,
    Chest(ChestModel),
    Shulker(ShulkerModel),
    /// A piston head, which moves from its reported extension toward its state's end.
    PistonArm {
        facing: u8,
        sticky: bool,
        extension: f32,
        target: f32,
    },
    Sign {
        mount: SignMount,
        front: Option<SignTextSpec>,
        back: Option<SignTextSpec>,
    },
    EnchantTable,
    /// The item overlays of an overlay host, by Bedrock face; its block-state overlays are
    /// drawn with the terrain mesh.
    OverlayItems(Box<[(u8, world::OverlayItem)]>),
}

/// Plans the item overlays of an overlay host's `overlays`, resolved by `ids`, while `backing`,
/// the cell's block, is their host; `None` when it draws no item.
pub(super) fn overlay_items(
    overlays: &world::CellOverlays,
    ids: &dyn world::BlockIds,
    backing: u32,
) -> Option<Template> {
    let resolved = overlays.resolve(ids);
    let items: Box<[_]> = resolved
        .on(backing)?
        .iter()
        .filter_map(|overlay| match &overlay.look {
            world::OverlayLook::Item(item) => Some((overlay.face, item.clone())),
            world::OverlayLook::Block(_) => None,
        })
        .collect();
    (!items.is_empty()).then_some(Template::OverlayItems(items))
}

/// Facing from either state spelling: `minecraft:cardinal_direction` or `facing_direction`.
fn facing(state: &BlockState) -> Option<Facing> {
    state
        .text(BlockProperty::MinecraftCardinalDirection.name())
        .and_then(Facing::from_cardinal)
        .or_else(|| {
            state
                .int("facing_direction")
                .and_then(Facing::from_facing_direction)
        })
}

/// Sixteen-step ground rotation in degrees, 0 facing south.
fn ground_rotation(state: &BlockState) -> f32 {
    state
        .int("ground_sign_direction")
        .unwrap_or(0)
        .rem_euclid(16) as f32
        * 22.5
}

/// Reads modern degree rotations and legacy integral turns, defaulting unusable data to zero.
fn frame_rotation(nbt: &NbtCompound) -> f32 {
    if let Some(turns) = nbt.integer("ItemRotation") {
        return turns.rem_euclid(8) as f32 * ITEM_FRAME_ROTATION_STEP_DEGREES;
    }
    nbt.float("ItemRotation")
        .filter(|degrees| degrees.is_finite())
        .map(|degrees| degrees.rem_euclid(8.0 * ITEM_FRAME_ROTATION_STEP_DEGREES))
        .unwrap_or(0.0)
}

/// Selects one leading half from a valid pair.
fn chest_pair(position: [i32; 3], nbt: &NbtCompound) -> ChestPair {
    let (Some(pair_x), Some(pair_z)) = (nbt.integer("pairx"), nbt.integer("pairz")) else {
        return ChestPair::Single;
    };
    let (Ok(pair_x), Ok(pair_z)) = (i32::try_from(pair_x), i32::try_from(pair_z)) else {
        return ChestPair::Single;
    };
    // Without an explicit lead flag the lower coordinate leads, so exactly one half draws.
    let lead = nbt
        .boolean("pairlead")
        .unwrap_or((position[0], position[2]) < (pair_x, pair_z));
    if lead {
        ChestPair::Lead {
            partner: [pair_x, position[1], pair_z],
        }
    } else {
        ChestPair::Follower
    }
}

/// Builds banner layers for the compiled mount.
fn banner(
    backing: BlockEntityBacking<'_>,
    state: &BlockState,
    nbt: &NbtCompound,
) -> Option<Template> {
    let mount = match backing.mount {
        BlockEntityMount::Standing => BannerMount::Standing {
            rotation_degrees: ground_rotation(state),
        },
        BlockEntityMount::Wall => BannerMount::Wall(facing(state)?),
        _ => return None,
    };
    let layers = nbt
        .list("Patterns")
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            let world::NbtValue::Compound(entry) = entry else {
                return None;
            };
            Some(BannerLayer {
                pattern: pattern_texture(entry.string("Pattern")?)?,
                color: banner_color(entry.integer("Color")?),
            })
        })
        .take(MAX_BANNER_LAYERS)
        .collect();
    Some(Template::Static(BlockEntityKind::Banner(BannerModel {
        mount,
        base: banner_color(nbt.integer("Base").unwrap_or(15)),
        layers,
    })))
}

/// Resolves the compiled mount using the retained state's facing and rotation.
fn sign_mount(backing: BlockEntityBacking<'_>, state: &BlockState) -> Option<SignMount> {
    Some(match backing.mount {
        BlockEntityMount::Hanging => {
            if state.int(BlockProperty::AttachedBit.name()) == Some(1) {
                SignMount::Hanging {
                    rotation_degrees: ground_rotation(state),
                }
            } else {
                SignMount::HangingWall(facing(state)?)
            }
        }
        BlockEntityMount::Wall => SignMount::Wall(facing(state)?),
        BlockEntityMount::Standing => SignMount::Standing {
            rotation_degrees: ground_rotation(state),
        },
        BlockEntityMount::None => return None,
    })
}

/// Plans one bed half from its block state and the dye id its block entity names.
/// Without a dye the bed shows the default colour; an unknown dye id draws nothing.
pub(super) fn bed(state: &BlockState, dye: Option<i64>) -> Option<BedModel> {
    Some(BedModel {
        color: bed_color(dye.unwrap_or(DEFAULT_BED_DYE))?,
        head: state.int(BlockProperty::HeadPieceBit.name()) == Some(1),
        direction: u8::try_from(state.int("direction")?.rem_euclid(4)).ok()?,
    })
}

/// Plans a piston head from its block, and from its actor data when the server sent any; a piston
/// without actor data shows a retracted head.
pub(super) fn piston_arm(
    name: &str,
    state: &BlockState,
    nbt: Option<&NbtCompound>,
) -> Option<Template> {
    let facing = u8::try_from(state.int(BlockProperty::FacingDirection.name())?).ok()?;
    if usize::from(facing) >= assets::block_shapes::PISTON_DIRECTIONS.len() {
        return None;
    }
    let extension = nbt
        .and_then(|nbt| nbt.float("Progress"))
        .filter(|progress| (0.0..=1.0).contains(progress))
        .unwrap_or(0.0);
    let piston_state = nbt.and_then(|nbt| nbt.integer("NewState").or_else(|| nbt.integer("State")));
    let target = match piston_state {
        Some(1 | 2) => 1.0,
        Some(_) => 0.0,
        None => extension,
    };
    let sticky = nbt
        .and_then(|nbt| nbt.boolean("Sticky"))
        .unwrap_or(name == assets::facts::block::STICKY_PISTON_IDENTIFIER);
    Some(Template::PistonArm {
        facing,
        sticky,
        extension,
        target,
    })
}

/// Keeps visible text and its retained appearance.
fn sign_face(face: &NbtCompound) -> Option<SignTextSpec> {
    let spec = SignTextSpec::new(
        face.string("Text")?.to_owned(),
        face.integer("SignTextColor")
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(DEFAULT_SIGN_COLOR),
        face.boolean("IgnoreLighting").unwrap_or(false),
        face.boolean("HideGlowOutline").unwrap_or(false),
    );
    spec.is_visible().then_some(spec)
}

/// Plans the visible faces of a sign on its compiled mount.
fn sign(
    backing: BlockEntityBacking<'_>,
    state: &BlockState,
    nbt: &NbtCompound,
) -> Option<Template> {
    let mount = sign_mount(backing, state)?;
    let (front, back) = match (nbt.compound("FrontText"), nbt.compound("BackText")) {
        (None, None) => (
            // Pre-1.19.80 signs carry one face's text at the root.
            sign_face(nbt),
            None,
        ),
        (front, back) => (front.and_then(sign_face), back.and_then(sign_face)),
    };
    (front.is_some() || back.is_some()).then_some(Template::Sign { mount, front, back })
}

/// Plans the draw for one block entity, or `None` when nothing should be drawn.
pub(super) fn describe(
    id: &str,
    backing: BlockEntityBacking<'_>,
    block_name: &str,
    state: &BlockState,
    nbt: &NbtCompound,
    position: [i32; 3],
) -> Option<Template> {
    if backing.entity.nbt_id != id {
        return None;
    }
    match backing.entity.renderer {
        BlockEntityRenderer::Chest => Some(Template::Chest(ChestModel {
            variant: ChestVariant::from_block_name(block_name)?,
            facing: facing(state).unwrap_or(Facing::North),
            pair: chest_pair(position, nbt),
            lid: 0.0,
        })),
        BlockEntityRenderer::EnderChest => Some(Template::Chest(ChestModel {
            variant: ChestVariant::Ender,
            facing: facing(state).unwrap_or(Facing::North),
            pair: ChestPair::Single,
            lid: 0.0,
        })),
        BlockEntityRenderer::Shulker => Some(Template::Shulker(ShulkerModel {
            color: shulker_color_from_block_name(block_name)?,
            facing: nbt
                .integer("facing")
                .and_then(|value| u8::try_from(value).ok())
                .filter(|value| *value < 6)
                .unwrap_or(1),
            open: 0.0,
        })),
        BlockEntityRenderer::Skull => {
            let kind = SkullKind::from_block_identifier(block_name)?;
            let mount = match state
                .int("facing_direction")
                .and_then(Facing::from_facing_direction)
            {
                Some(wall) => SkullMount::Wall(wall),
                None => SkullMount::Floor {
                    rotation_degrees: nbt.float("Rotation").unwrap_or(0.0),
                },
            };
            Some(Template::Static(BlockEntityKind::Skull(SkullModel {
                kind,
                mount,
            })))
        }
        BlockEntityRenderer::Bed => bed(state, Some(nbt.integer("color")?))
            .map(|model| Template::Static(BlockEntityKind::Bed(model))),
        BlockEntityRenderer::Chalkboard => super::chalkboard::model(state, nbt, position)
            .map(|model| Template::Static(BlockEntityKind::Chalkboard(model))),
        BlockEntityRenderer::Banner => banner(backing, state, nbt),
        BlockEntityRenderer::Sign => sign(backing, state, nbt),
        BlockEntityRenderer::EnchantTable => Some(Template::EnchantTable),
        BlockEntityRenderer::Lectern => nbt.boolean("hasBook").unwrap_or(false).then(|| {
            Template::Static(BlockEntityKind::Lectern {
                facing_yaw_degrees: facing(state).unwrap_or(Facing::North).yaw_degrees(),
            })
        }),
        BlockEntityRenderer::PistonArm => piston_arm(block_name, state, Some(nbt)),
        BlockEntityRenderer::Bell => Some(Template::Bell {
            attachment: state
                .text("attachment")
                .and_then(BellAttachment::from_state)
                .unwrap_or(BellAttachment::Standing),
            ring_direction: nbt
                .integer("Direction")
                .and_then(|value| u8::try_from(value.rem_euclid(4)).ok())
                .unwrap_or(0),
        }),
        BlockEntityRenderer::Beacon => {
            (nbt.integer("Levels").unwrap_or(0) > 0).then_some(Template::Beacon)
        }
        BlockEntityRenderer::ItemFrame | BlockEntityRenderer::GlowItemFrame => {
            let outward = u8::try_from(state.int("facing_direction")?).ok()?;
            (outward < 6).then(|| Template::ItemFrame {
                model: ItemFrameModel {
                    glow: backing.entity.renderer == BlockEntityRenderer::GlowItemFrame,
                    outward,
                    full_face: [
                        BlockProperty::ItemFrameMapBit,
                        BlockProperty::ItemFramePhotoBit,
                    ]
                    .into_iter()
                    .any(|bit| state.int(bit.name()).is_some_and(|value| value != 0)),
                    map: None,
                },
                item: nbt.compound("Item").and_then(held_item),
                map_id: nbt
                    .compound("Item")
                    .and_then(|stack| stack.compound("tag"))
                    .and_then(|tag| tag.integer("map_uuid")),
                rotation_degrees: frame_rotation(nbt),
            })
        }
        BlockEntityRenderer::FlowerPot => nbt
            .compound("PlantBlock")
            .and_then(plant_block)
            .map(|plant| Template::FlowerPot { plant }),
        BlockEntityRenderer::Campfire => Some(Template::Campfire {
            yaw_degrees: facing(state).unwrap_or(Facing::North).yaw_degrees(),
            items: std::array::from_fn(|slot| {
                nbt.compound(&format!("Item{}", slot + 1))
                    .and_then(held_item)
            }),
        }),
        BlockEntityRenderer::Conduit => Some(Template::Conduit {
            active: nbt.boolean("Active").unwrap_or(false),
            hunting: nbt.integer("Target").is_some_and(|target| target != -1),
        }),
        BlockEntityRenderer::DecoratedPot => {
            let mut sherds: [Option<String>; 4] = Default::default();
            for (slot, entry) in nbt
                .list("sherds")
                .unwrap_or_default()
                .iter()
                .take(4)
                .enumerate()
            {
                if let world::NbtValue::String(item) = entry {
                    sherds[slot] = sherd_pattern(item);
                }
            }
            Some(Template::Static(BlockEntityKind::DecoratedPot(
                DecoratedPotModel {
                    facing: facing(state).unwrap_or(Facing::North),
                    sherds,
                },
            )))
        }
        BlockEntityRenderer::Statue => {
            Some(Template::Static(BlockEntityKind::Statue(StatueModel {
                pose: StatuePose::from_nbt(nbt.string("Pose"), nbt.integer("Pose"))?,
                oxidation: Oxidation::from_block_name(block_name)?,
                facing: facing(state).unwrap_or(Facing::North),
            })))
        }
        BlockEntityRenderer::EndPortal => Some(Template::Static(BlockEntityKind::EndPortal)),
        BlockEntityRenderer::EndGateway => Some(Template::Static(BlockEntityKind::EndGateway)),
        BlockEntityRenderer::None | BlockEntityRenderer::MobSpawner => None,
    }
}

#[cfg(test)]
mod tests;
