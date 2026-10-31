//! Real held meshes and authored hand placements, independent of GUI icon projection.

use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use assets::{
    EquipmentCategory, ItemDisplayScalar, ItemVisualDefinitionRoute, ItemVisualKey,
    RuntimeEntityAssets, RuntimeEquipmentCatalog, RuntimeIconCatalog,
};
use render_model::{RenderBoneTransform, held_sprite_vertices, textured_cube_vertices};

use player_preview::{PreviewHeldModel, PreviewHeldPlacement};
use {
    super::{UiPresentationError, atlas, player_preview},
    ui::IconRef,
};

/// Prepared variants support borrowed item-name lookups without allocating a temporary key.
pub(super) type HeldModels = BTreeMap<Box<str>, BTreeMap<u32, HeldModel>>;

/// One item's held model. A sprite item's extruded mesh is built the first time a preview holds
/// it, since every item has one and most are never held.
pub(super) struct HeldModel {
    model: PreviewHeldModel,
    /// An unrestricted authored model keeps the ordinary sprite available as a fallback.
    authored: Option<PreviewHeldModel>,
    /// The item sprite extruded on first use; `None` for models built up front.
    sprite: Option<usize>,
    extruded: OnceLock<Option<Arc<[render_model::ActorRigVertex]>>>,
}

impl HeldModel {
    /// Whether the held geometry follows an item sprite rather than authored or block geometry.
    pub(super) const fn is_sprite(&self) -> bool {
        self.authored.is_none() && self.sprite.is_some()
    }

    /// Changes a sprite's artwork and mask while retaining its hand placement and glint policy.
    pub(super) fn with_sprite(
        &self,
        source: IconRef,
        vertices: Arc<[render_model::ActorRigVertex]>,
    ) -> PreviewHeldModel {
        PreviewHeldModel {
            source: source.with_glint(self.model.source.glint),
            vertices,
            ..self.model.clone()
        }
    }

    /// Admits an already prepared held mesh without lazy sprite extrusion.
    pub(super) const fn ready(model: PreviewHeldModel) -> Self {
        Self {
            model,
            authored: None,
            sprite: None,
            extruded: OnceLock::new(),
        }
    }

    /// The model with its mesh, extruding a sprite item's sprite from `icons` on first use;
    /// `None` when that sprite has no mesh.
    pub(super) fn model(&self, icons: &RuntimeIconCatalog) -> Option<PreviewHeldModel> {
        if let Some(model) = &self.authored {
            return Some(model.clone());
        }
        if self.sprite.is_none() {
            return Some(self.model.clone());
        }
        self.sprite_model(icons)
    }

    /// Extrudes the ordinary sprite once, including when an authored model takes precedence.
    fn sprite_model(&self, icons: &RuntimeIconCatalog) -> Option<PreviewHeldModel> {
        let sprite = self.sprite?;
        let vertices = self
            .extruded
            .get_or_init(|| {
                let sprite = icons.sprites().get(sprite)?;
                held_sprite_vertices(
                    usize::from(sprite.width),
                    usize::from(sprite.height),
                    &sprite.rgba8,
                    [0.0, 0.0, 1.0, 1.0],
                )
                .map(Into::into)
            })
            .clone()?;
        Some(PreviewHeldModel {
            vertices,
            ..self.model.clone()
        })
    }
}

pub(super) fn prepare(
    atlas: &mut atlas::Atlas,
    entities: &RuntimeEntityAssets,
    icons: &RuntimeIconCatalog,
    icon_refs: &[IconRef],
    equipment: Option<&RuntimeEquipmentCatalog>,
    blocks: &BTreeMap<u32, IconRef>,
) -> Result<HeldModels, UiPresentationError> {
    let Some(hand_pivots) = player_hands(entities) else {
        // A model without native item bones cannot safely manufacture a grip origin.
        return Ok(BTreeMap::new());
    };
    let mut models = HeldModels::new();
    for entry in icons.entries() {
        let sprite = icons
            .lookup_held_index(&entry.identifier, entry.metadata)
            .ok_or(UiPresentationError::InvalidFontTexture)?;
        let definition = entities.item_visuals().iter().find(|visual| {
            visual.key.identifier == entry.identifier && visual.key.metadata == entry.metadata
        });
        let model = match definition.map(|definition| definition.route) {
            Some(ItemVisualDefinitionRoute::BlockItem { block_visual })
                if blocks.contains_key(&block_visual.0) =>
            {
                Some((block(blocks[&block_visual.0], hand_pivots), None))
            }
            Some(ItemVisualDefinitionRoute::EmptyHand) => None,
            _ => {
                if icons.sprites().get(sprite).is_none() {
                    continue;
                }
                // Sprite texels already live in the item atlas; keep their original region.
                let source = *icon_refs
                    .get(sprite)
                    .ok_or(UiPresentationError::InvalidFontTexture)?;
                let model = PreviewHeldModel {
                    source,
                    hand_pivots,
                    vertices: Arc::from([]),
                    placements: [PreviewHeldPlacement::Sprite {
                        hand_equipped: definition
                            .and_then(|definition| definition.grip.upright())
                            .unwrap_or(false),
                    }; 2],
                };
                Some((model, Some(sprite)))
            }
        };
        if let Some((model, sprite)) = model {
            models.entry(entry.identifier.clone()).or_default().insert(
                entry.metadata,
                HeldModel {
                    sprite,
                    ..HeldModel::ready(model.with_glint(
                        Some(
                            definition.is_some_and(|definition| {
                                definition.glint.enabled().unwrap_or(false)
                            }),
                        ),
                    ))
                },
            );
        }
    }
    if let Some(equipment) = equipment {
        for binding in equipment
            .bindings()
            .iter()
            .filter(|binding| player_preview::selection::unrestricted(binding, Some(entities)))
        {
            let Some(texture) = equipment.texture(&binding.texture.identifier) else {
                continue;
            };
            let source = atlas.insert([texture.width, texture.height], &texture.rgba8)?;
            let Some(model) = authored(entities, equipment, binding, hand_pivots, source) else {
                continue;
            };
            let model = model.with_glint(Some(entities.item_visuals().iter().any(|definition| {
                definition.key.identifier == binding.identifier
                    && definition.key.metadata == 0
                    && definition.glint.enabled().unwrap_or(false)
            })));
            models
                .entry(binding.identifier.clone())
                .or_default()
                .entry(0)
                .and_modify(|held| held.authored = Some(model.clone()))
                .or_insert_with(|| HeldModel::ready(model));
        }
    }
    Ok(models)
}

/// Resolves the original player's right and left item bone pivots.
pub(super) fn player_hands(entities: &RuntimeEntityAssets) -> Option<[[f32; 3]; 2]> {
    let binding = entities.rig_bindings().iter().find(|binding| {
        entities
            .symbols()
            .get(binding.entity_symbol as usize)
            .is_some_and(|symbol| {
                symbol.kind == assets::EntityAssetKind::Entity
                    && assets::facts::entity::EntityFacts::pinned()
                        .get(&symbol.identifier)
                        .native_class
                        == assets::facts::entity::NativeEntityClass::Player
            })
    })?;
    let geometry = entities
        .rig_geometries()
        .get(binding.first_geometry as usize)?
        .geometry as usize;
    let names = render_model::geometry_bone_names(entities, geometry)?;
    let pivots = render_model::geometry_bone_pivots(entities, geometry)?;
    let hand = |name: &str| {
        let index = names
            .iter()
            .position(|bone| bone.eq_ignore_ascii_case(name))?;
        pivots.get(index).copied()
    };
    Some([hand("rightItem")?, hand("leftItem")?])
}

fn block(source: IconRef, hand_pivots: [[f32; 3]; 2]) -> PreviewHeldModel {
    // Share the same six-face cube and UV contract as the world held-item renderer.
    let faces = super::sheet_faces(source).map(|face| {
        [
            f32::from(face.uv[0] - source.uv[0]) / f32::from(source.uv[2] - source.uv[0]),
            f32::from(face.uv[1] - source.uv[1]) / f32::from(source.uv[3] - source.uv[1]),
            f32::from(face.uv[2] - source.uv[0]) / f32::from(source.uv[2] - source.uv[0]),
            f32::from(face.uv[3] - source.uv[1]) / f32::from(source.uv[3] - source.uv[1]),
        ]
    });
    PreviewHeldModel {
        source,
        hand_pivots,
        vertices: textured_cube_vertices(faces).into(),
        placements: [PreviewHeldPlacement::Block; 2],
    }
}

/// Builds the selected literal model using an already admitted artwork region.
fn authored(
    entities: &RuntimeEntityAssets,
    equipment: &RuntimeEquipmentCatalog,
    binding: &assets::EquipmentBinding,
    hand_pivots: [[f32; 3]; 2],
    source: IconRef,
) -> Option<PreviewHeldModel> {
    use render_model::equipment::{BoneChannels, attach};
    let channels = |off_hand: bool| -> Option<BoneChannels> {
        match binding.category {
            EquipmentCategory::Held | EquipmentCategory::ArmorAndHeld { .. } => {
                let transform = binding.third_person.literal()?;
                Some(BoneChannels {
                    translation: transform.translation.map(ItemDisplayScalar::get),
                    rotation: transform.rotation.map(ItemDisplayScalar::get),
                    scale: transform.scale.map(ItemDisplayScalar::get),
                })
            }
            EquipmentCategory::Shield => {
                let slot = if off_hand { "off_hand" } else { "main_hand" };
                let pose = binding.pose(&format!("wield_third_person@{slot}"))?;
                let [bone] = &*pose.bones else { return None };
                let channel = |value: Option<[ItemDisplayScalar; 3]>, rest: f32| {
                    value.map_or([rest; 3], |value| value.map(ItemDisplayScalar::get))
                };
                Some(BoneChannels {
                    translation: channel(bone.translation, 0.0),
                    rotation: channel(bone.rotation, 0.0),
                    scale: channel(bone.scale, 1.0),
                })
            }
            _ => None,
        }
    };
    let (Some(main), Some(off)) = (channels(false), channels(true)) else {
        return None;
    };
    let index = entities
        .geometries()
        .iter()
        .position(|geometry| geometry.identifier == binding.geometry.identifier)?;
    let Ok(bones) = render_model::resolve_geometry_bones(entities, index) else {
        return None;
    };
    let [root] = &*bones else {
        return None;
    };
    let texture = equipment.texture(&binding.texture.identifier)?;
    let Ok(geometry) = render_model::attachable_geometry(
        entities,
        index,
        render_model::item_mesh_rig_id(0),
        texture,
    ) else {
        return None;
    };
    let [pivot] = &*geometry.bone_pivots else {
        return None;
    };
    let identity = RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0, 0.0, 0.0, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let (Some(main), Some(off)) = (
        attach(identity, *pivot, main, root.binding.is_some()),
        attach(identity, *pivot, off, root.binding.is_some()),
    ) else {
        return None;
    };
    Some(PreviewHeldModel {
        source,
        hand_pivots,
        vertices: Arc::clone(&geometry.vertices),
        placements: [main, off].map(|bone| PreviewHeldPlacement::Authored {
            bone,
            pivot: *pivot,
        }),
    })
}

#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod variant_tests;

mod dressing;

/// One demanded authored definition; retaining the asset Arc prevents stale identity reuse.
pub(super) struct SelectedModel {
    pub item: ItemVisualKey,
    definition: Box<str>,
    entities: Arc<RuntimeEntityAssets>,
    pub(super) artwork: Arc<RuntimeEquipmentCatalog>,
    /// Missing geometry is cached; unavailable artwork retains prepared geometry for retry.
    pub model: Option<PreviewHeldModel>,
    /// Whether the current atlas admits this model's artwork.
    pub ready: bool,
}

impl SelectedModel {
    /// Reuses geometry only while its definition, source artifacts and hand frame are unchanged.
    fn matches(
        &self,
        binding: &assets::EquipmentBinding,
        entities: &Arc<RuntimeEntityAssets>,
        artwork: &Arc<RuntimeEquipmentCatalog>,
        hand_pivots: [[f32; 3]; 2],
    ) -> bool {
        self.definition.as_ref() == binding.attachable_identifier()
            && Arc::ptr_eq(&self.entities, entities)
            && Arc::ptr_eq(&self.artwork, artwork)
            && self
                .model
                .as_ref()
                .is_none_or(|model| model.hand_pivots == hand_pivots)
    }
}
