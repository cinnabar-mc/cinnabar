//! Demand-bound authored hand models use the preview's selected definition and texture pages.
use super::*;
use crate::ui_runtime::presentation::{
    UiPresentationRuntime,
    player_preview::{PreviewEquipmentOwner, PreviewHandItem},
};

impl UiPresentationRuntime {
    /// Selects both hands with real stack inputs and retains at most one authored model per hand.
    pub(in crate::ui_runtime::presentation) fn prepare_preview_hands(
        &mut self,
        hands: &[Option<(PreviewHandItem, bool)>; 2],
        owner: Option<PreviewEquipmentOwner<'_>>,
        worn: [Option<&str>; 4],
    ) {
        let base = self.equipment_catalog.clone();
        let pack = self.gui_models.pack_equipment.source.clone();
        let selected: [_; 2] = std::array::from_fn(|index| {
            let (hand, enchanted) = hands[index].as_ref()?;
            let mut budget = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK;
            let owner = owner.map(|owner| PreviewEquipmentOwner {
                input: owner.input.for_hand(index == 1),
                ..owner
            });
            let selection = self.preview_equipment_selection.binding(
                base.as_deref(),
                pack.as_deref(),
                &hand.identifier,
                *enchanted,
                owner,
                None,
                &mut budget,
            )?;
            let binding = selection.binding;
            let mut entities = self
                .preview_equipment_selection
                .entities(selection.from_pack)?;
            if render_model::find_geometry_index(&entities, &binding.geometry.identifier).is_none()
            {
                entities = self.preview_equipment_selection.entities(false)?;
            }
            let artwork = pack
                .as_ref()
                .filter(|pack| pack.texture(&binding.texture.identifier).is_some())
                .or_else(|| {
                    base.as_ref()
                        .filter(|base| base.texture(&binding.texture.identifier).is_some())
                })?;
            let hand_pivots = self.gui_models.sources.hand_pivots?;
            let model = if let Some(previous) = self.gui_models.selected_held[index]
                .as_ref()
                .filter(|previous| previous.matches(binding, &entities, artwork, hand_pivots))
            {
                previous.model.clone()
            } else {
                let texture = artwork.texture(&binding.texture.identifier)?;
                // Geometry must be drawable before its artwork can consume an atlas region.
                authored(
                    &entities,
                    artwork,
                    binding,
                    hand_pivots,
                    IconRef {
                        color_mask: None,
                        page: 0,
                        uv: [0, 0, texture.width, texture.height],
                        glint: false,
                    },
                )
            };
            Some((binding, entities, Arc::clone(artwork), model))
        });
        self.prepare_shield_hands(selected.iter().enumerate().filter_map(|(index, selected)| {
            let (binding, _, artwork, model) = selected.as_ref()?;
            model.as_ref()?;
            let hand = &hands[index].as_ref()?.0;
            Some((
                hand.shield_pattern.as_deref()?,
                artwork.texture(&binding.texture.identifier)?,
            ))
        }));
        self.wear_pack_equipment(
            worn,
            selected.each_ref().map(|selected| {
                let (binding, _, artwork, model) = selected.as_ref()?;
                model.as_ref()?;
                pack.as_ref().filter(|pack| Arc::ptr_eq(pack, artwork))?;
                Some(binding.texture.identifier.as_ref())
            }),
        );
        for (index, selected) in selected.into_iter().enumerate() {
            let previous = self.gui_models.selected_held[index].take();
            self.gui_models.selected_held[index] = (|| {
                let (binding, entities, artwork, mut model) = selected?;
                let hand = &hands[index].as_ref()?.0;
                let hand_pivots = self.gui_models.sources.hand_pivots?;
                let texture = artwork.texture(&binding.texture.identifier)?;
                let source = model.as_ref().and_then(|_| {
                    if pack
                        .as_ref()
                        .is_some_and(|pack| Arc::ptr_eq(pack, &artwork))
                    {
                        self.gui_models
                            .pack_equipment
                            .regions
                            .get(&binding.texture.identifier)
                            .copied()
                    } else {
                        self.gui_models
                            .sources
                            .textures
                            .get(&atlas::key([texture.width, texture.height], &texture.rgba8))
                            .copied()
                    }
                });
                let source = hand
                    .shield_pattern
                    .as_deref()
                    .filter(|_| model.is_some())
                    .and_then(|pattern| self.shield_texture(pattern, texture))
                    .or(source);
                let ready = source.is_some();
                if let (Some(source), Some(model)) = (source, model.as_mut()) {
                    model.source =
                        source.with_glint(entities.item_visuals().iter().any(|visual| {
                            visual.key.identifier.as_ref() == hand.identifier.as_ref()
                                && visual.key.metadata == hand.metadata
                                && visual.glint.enabled().unwrap_or(false)
                        }));
                }
                if let Some(mut previous) = previous
                    && previous.item.identifier.as_ref() == hand.identifier.as_ref()
                    && previous.item.metadata == hand.metadata
                    && previous.matches(binding, &entities, &artwork, hand_pivots)
                {
                    previous.model = model;
                    previous.ready = ready;
                    return Some(previous);
                }
                Some(SelectedModel {
                    item: ItemVisualKey {
                        identifier: hand.identifier.as_ref().into(),
                        metadata: hand.metadata,
                    },
                    artwork,
                    definition: binding.attachable_identifier().into(),
                    entities,
                    ready,
                    model,
                })
            })();
        }
    }

    /// Resolves cached source geometry without projecting or allocating UI vertices.
    pub(in crate::ui_runtime::presentation::gui_models) fn preview_held_models(
        &self,
        include_held: [bool; 2],
    ) -> [Option<player_preview::PreviewHeldModel>; 2] {
        std::array::from_fn(|index| {
            let hand = &self.player_preview_gear.hands[index];
            if !include_held[index] {
                return None;
            }
            let hand = hand.as_ref()?;
            let pack_model_suppressed = !self.player_preview_gear.show_custom_item_models
                && self
                    .gui_models
                    .pack_equipment
                    .source
                    .as_ref()
                    .is_some_and(|pack| {
                        self.gui_models.selected_held[index]
                            .as_ref()
                            .is_some_and(|selected| Arc::ptr_eq(&selected.artwork, pack))
                    });
            if let Some(selected) = self.gui_models.selected_held[index]
                .as_ref()
                .filter(|_| !pack_model_suppressed)
                && selected.ready
                && selected.item.identifier.as_ref() == hand.identifier.as_ref()
                && selected.item.metadata == hand.metadata
            {
                return Some(
                    selected
                        .model
                        .as_ref()?
                        .with_grip(hand.grip)
                        .with_glint(hand.glint),
                );
            }
            let (identifier, metadata) = Self::item_icon_key(
                &hand.identifier,
                hand.metadata,
                hand.use_animation,
                hand.charged_projectile,
                None,
            );
            let held = |metadata| self.gui_models.sources.held.get(identifier)?.get(&metadata);
            let base = held(metadata).or_else(|| held(0));
            let sprite = hand
                .shield_pattern
                .as_deref()
                .and_then(|pattern| self.shield_preview_sprite(pattern));
            let model = if !pack_model_suppressed
                && sprite.is_none()
                && let Some((source, vertices)) =
                    self.session_icons.held_block(identifier, metadata)
            {
                player_preview::PreviewHeldModel {
                    source,
                    vertices,
                    hand_pivots: self.gui_models.sources.hand_pivots?,
                    placements: [player_preview::PreviewHeldPlacement::Block; 2],
                }
            } else if sprite.is_some()
                || ((pack_model_suppressed || base.is_none_or(HeldModel::is_sprite))
                    && self.session_item_icon(identifier, metadata).is_some())
            {
                let (source, vertices) =
                    sprite.or_else(|| self.session_icons.held_sprite(identifier, metadata))?;
                match base.filter(|base| base.is_sprite()) {
                    Some(base) => base.with_sprite(source, vertices),
                    None => player_preview::PreviewHeldModel {
                        source,
                        vertices,
                        hand_pivots: self.gui_models.sources.hand_pivots?,
                        placements: [player_preview::PreviewHeldPlacement::Sprite {
                            hand_equipped: false,
                        }; 2],
                    },
                }
            } else {
                let icons = self.icon_catalog.as_deref()?;
                if pack_model_suppressed {
                    base?.sprite_model(icons)?
                } else {
                    base?.model(icons)?
                }
            };
            Some(model.with_grip(hand.grip).with_glint(hand.glint))
        })
    }
}
