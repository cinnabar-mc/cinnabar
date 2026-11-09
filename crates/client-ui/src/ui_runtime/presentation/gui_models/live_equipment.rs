//! Live controls consume evaluated attachables while standalone previews retain static models.
use super::{super::UiPresentationRuntime, live_player::ModelGeometry};
use actor_animation::{
    AttachableAnimationInput,
    attachment::{compose_parent, parent_for_root},
};
use render_model::{ActorRigGeometry, EntityRigId, UiTexturePage};
use std::sync::Arc;
use ui::IconRef;

#[derive(Default)]
pub(in super::super) struct LiveEquipment {
    pub(super) input: Option<AttachableAnimationInput<'static>>,
    /// A successful inactive draw suppresses the ordinary preview fallback.
    pub(super) hands: [Option<Vec<HeldLayer>>; 2],
    pub(in super::super) pages: Vec<UiTexturePage>,
}

pub(super) struct HeldLayer {
    key: ([u8; 32], u32, Box<str>),
    page: UiTexturePage,
    pub(super) model: ModelGeometry,
    pub(super) layer: actor_animation::RenderTextureLayer,
    pub(super) icon: IconRef,
}

impl UiPresentationRuntime {
    /// Evaluates selected held definitions with UI queries and the captured actor's item bones.
    pub(super) fn capture_live_equipment(
        &mut self,
        local: Option<client_world::ui_read::LocalPlayerUi<'_>>,
    ) {
        let previous = std::mem::take(&mut self.gui_models.live_equipment.hands);
        let mut pages = Vec::new();
        let first = self.textures.dynamic_start()
            + render_model::UI_SELECTED_MODEL_PAGE_OFFSET
            + self.gui_models.player_layers.pages.len();
        let mut bytes: usize = self
            .gui_models
            .player_layers
            .pages
            .iter()
            .map(|page| page.pixels().len())
            .sum();
        let hands = &self.player_preview_gear.hands;
        let base = self.equipment_catalog.as_deref();
        let pack = self.gui_models.pack_equipment.source.as_deref();
        let live = &self.gui_models.live_player;
        let mut draws = std::array::from_fn(|hand| {
            let item = hands[hand].as_ref()?;
            let local = local?;
            let actor = local.actor()?;
            let rig = local.ui_rig()?;
            let enchantment = [(
                assets::entity::EngineVariable::IsEnchanted.name(),
                f32::from(item.glint.unwrap_or(false)),
            )];
            let input = AttachableAnimationInput {
                first_person: false,
                is_in_ui: true,
                is_paperdoll: false,
                is_local_player: true,
                off_hand: hand == 1,
                held_item_scale: rig.held_item_scale,
                owner_main_hand: hands[0].as_ref().map(|item| item.identifier.as_ref()),
                owner_off_hand: hands[1].as_ref().map(|item| item.identifier.as_ref()),
                owner_variables: &enchantment,
                ..self
                    .gui_models
                    .live_equipment
                    .input
                    .unwrap_or_default()
                    .for_hand(hand == 1)
            };
            let mut budget = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK;
            let selected = self.preview_equipment_selection.binding(
                base,
                pack,
                &item.identifier,
                item.glint.unwrap_or(false),
                Some(super::super::player_preview::PreviewEquipmentOwner { actor, rig, input }),
                None,
                &mut budget,
            )?;
            if selected.from_pack && !self.player_preview_gear.show_custom_item_models {
                return None;
            }
            let assets = self
                .preview_equipment_selection
                .entities(selected.from_pack)?;
            assets.attachable_rig_binding(selected.binding.attachable_identifier())?;
            let evaluated = self.preview_equipment_selection.evaluate(
                selected.from_pack,
                selected.binding.attachable_identifier(),
                actor,
                &rig,
                input,
            )?;
            let inactive = evaluated.render.is_empty();
            let mut draws = Vec::new();
            for layer in evaluated.render {
                let geometry = layer.geometry.unwrap_or(evaluated.geometry);
                let Some(path) = assets
                    .sources()
                    .get(layer.source as usize)
                    .map(|source| source.path.as_ref())
                else {
                    continue;
                };
                let texture_id = path
                    .strip_suffix(".png")
                    .or_else(|| path.strip_suffix(".tga"))
                    .unwrap_or(path);
                let Some(texture) = pack
                    .and_then(|catalog| catalog.texture(texture_id))
                    .or_else(|| base.and_then(|catalog| catalog.texture(texture_id)))
                else {
                    continue;
                };
                let key = (
                    assets.source_manifest_sha256(),
                    geometry,
                    texture.identifier.clone(),
                );
                let cached = previous[hand]
                    .as_ref()
                    .and_then(|draws| draws.iter().find(|draw| draw.key == key));
                let geometry_source: Option<ActorRigGeometry> = cached
                    .and_then(|draw| draw.model.geometry.clone())
                    .or_else(|| {
                        render_model::attachable_geometry(
                            &assets,
                            geometry as usize,
                            EntityRigId(rig.rig.0),
                            texture,
                        )
                        .ok()
                    });
                let Some(geometry_source) = geometry_source else {
                    continue;
                };
                let pose = if layer.pose.is_empty() {
                    evaluated.pose
                } else {
                    &layer.pose
                };
                let scale = evaluated
                    .axis_scale
                    .map(|axis| axis * evaluated.scale * input.main_hand_scale());
                let pose = pose
                    .iter()
                    .enumerate()
                    .map(|(index, bone)| {
                        compose_parent(
                            parent_for_root(
                                &live.bones,
                                &live.bones,
                                rig.bone_names,
                                input,
                                evaluated.bone_parent(geometry, index)?,
                                scale,
                            )?,
                            *bone,
                        )
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(pose) = pose else { continue };
                let mut model = ModelGeometry::default();
                model.capture(&geometry_source, &pose, live.frame);
                if model.geometry.is_none() {
                    continue;
                }
                let page = cached.map(|draw| draw.page.clone()).or_else(|| {
                    UiTexturePage::owned(
                        [texture.width.into(), texture.height.into()],
                        Arc::clone(&texture.rgba8),
                    )
                    .ok()
                });
                let Some(page) = page else {
                    continue;
                };
                let index = pages
                    .iter()
                    .position(|old| old == &page)
                    .unwrap_or(pages.len());
                if first + index >= render_model::MAX_UI_TEXTURE_LAYERS as usize {
                    continue;
                }
                if index == pages.len() {
                    if bytes + page.pixels().len() > render_model::MAX_UI_NATIVE_SHEET_BYTES {
                        continue;
                    }
                    bytes += page.pixels().len();
                    pages.push(page.clone());
                }
                draws.push(HeldLayer {
                    key,
                    page,
                    model,
                    layer: layer.clone(),
                    icon: IconRef {
                        page: (first + index) as u16,
                        uv: [0, 0, texture.width, texture.height],
                        color_mask: None,
                        glint: item.glint.unwrap_or(false),
                    },
                });
            }
            (inactive || !draws.is_empty()).then_some(draws)
        });
        for (hand, layers) in draws.iter_mut().enumerate() {
            let Some(pattern) = hands[hand]
                .as_ref()
                .and_then(|item| item.shield_pattern.as_deref())
            else {
                continue;
            };
            for draw in layers.iter_mut().flatten() {
                let texture = pack
                    .and_then(|catalog| catalog.texture(&draw.key.2))
                    .or_else(|| base.and_then(|catalog| catalog.texture(&draw.key.2)));
                if let Some(icon) =
                    texture.and_then(|texture| self.shield_texture(pattern, texture))
                {
                    draw.icon = icon.with_glint(draw.icon.glint);
                }
            }
        }
        self.gui_models.live_equipment.hands = draws;
        if self.gui_models.live_equipment.pages != pages {
            self.gui_models.live_equipment.pages = pages;
            self.rebuild_dynamic_textures();
        }
        self.publish_shield_icons();
        if !self
            .gui_models
            .live_equipment
            .pages
            .iter()
            .enumerate()
            .all(|(index, page)| self.textures.pages().get(first + index) == Some(page))
        {
            for hand in &mut self.gui_models.live_equipment.hands {
                if hand.as_ref().is_some_and(|layers| !layers.is_empty()) {
                    *hand = None;
                }
            }
        }
    }
}
