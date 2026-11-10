//! Captured actor draws share the retained HUD palette and equipment geometry.

use super::*;

impl UiPresentationRuntime {
    /// Draws the HUD doll from the live actor.
    #[cfg(test)]
    pub(in super::super) fn hud_player_mesh(
        &self,
        held: [Option<PreviewHeldModel>; 2],
        admit: impl FnMut(&HudModel, [bool; 2], usize, usize) -> bool,
    ) -> Option<Arc<UiMesh>> {
        self.live_player_mesh(PreviewView::Hud, true, held, admit)
    }

    /// Draws the captured actor with the selected view and its own controller context.
    pub(in super::super) fn live_player_mesh(
        &self,
        view: PreviewView,
        in_hud: bool,
        held: [Option<PreviewHeldModel>; 2],
        admit: impl FnMut(&HudModel, [bool; 2], usize, usize) -> bool,
    ) -> Option<Arc<UiMesh>> {
        let live = &self.gui_models.live_player;
        let controllers = &self.gui_models.player_layers;
        let ui_context = matches!(view, PreviewView::Live { .. });
        let body_visible = !ui_context || !controllers.visibility.hides_body();
        let held = if ui_context && controllers.visibility.hides_held_items() {
            [None, None]
        } else {
            held
        };
        let skin = self.gui_models.skin.as_ref()?;
        let [width, height] = skin.dimensions();
        let icon = IconRef {
            color_mask: None,
            page: (self.textures.dynamic_start() + super::super::SKIN_PAGE) as u16,
            uv: [0, 0, width.try_into().ok()?, height.try_into().ok()?],
            glint: false,
        };
        let mut cache = self
            .gui_models
            .doll_model(in_hud)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cache.begin();
        let mut rig = Rig::new(
            self.player_preview_pose.unwrap_or_default(),
            view,
            self.player_preview_bob,
            held.each_ref().map(Option::is_some),
        );
        rig.parts = live.parts;
        let turn = (view != PreviewView::Hud)
            .then(|| {
                let (pitch, yaw) = rig.head_look();
                live.head_turn(pitch, yaw)
            })
            .flatten();
        if let (Some(turn), Some(head)) = (turn, rig.parts[0].as_mut()) {
            *head = turn * *head;
        }
        let (screen, light) = projection(&rig, 6);
        let mut model = |model: &super::super::live_player::ModelGeometry,
                         icon: IconRef,
                         turned: &dyn Fn(usize) -> bool,
                         layer: Option<&actor_animation::RenderTextureLayer>,
                         hand: Option<usize>|
         -> Option<()> {
            let geometry = model.geometry.as_ref()?;
            let posed = |(bone, frame): (usize, &Affine3A)| {
                if layer
                    .is_some_and(|layer| layer.hidden_bones.binary_search(&(bone as u32)).is_ok())
                {
                    return Affine3A::from_scale(Vec3::ZERO);
                }
                match turn.filter(|_| turned(bone)) {
                    Some(turn) => turn * *frame,
                    None => *frame,
                }
            };
            let start = cache.bones.len();
            cache.layer(
                &geometry.vertices,
                model
                    .transforms
                    .iter()
                    .enumerate()
                    .map(|bone| screen * Mat4::from(posed(bone))),
                model
                    .transforms
                    .iter()
                    .enumerate()
                    .map(&posed)
                    .map(|frame| {
                        if layer.is_some_and(|layer| layer.ignore_lighting) {
                            Mat3::ZERO
                        } else {
                            normal_frame(light * Mat3::from(frame.matrix3), true)
                        }
                    }),
                icon,
                None,
                layer.map_or(live.overlay_color, |layer| layer.overlay),
            )?;
            if let Some(layer) = layer {
                cache.style_last_layer(start, layer);
            }
            let index = cache.cursor - 1;
            cache.sources[index].hand = hand;
            Some(())
        };
        if body_visible {
            let turned = |bone| live.head_bones.get(bone).copied().unwrap_or(false);
            if ui_context
                && controllers
                    .live
                    .iter()
                    .any(|draw| draw.layer.geometry.is_none() || draw.model.is_some())
            {
                for draw in &controllers.live {
                    let source = if draw.layer.geometry.is_some() {
                        let Some(source) = &draw.model else { continue };
                        source
                    } else {
                        &live.body
                    };
                    model(
                        source,
                        draw.icon.unwrap_or(icon),
                        &|bone| draw.layer.geometry.is_none() && turned(bone),
                        Some(&draw.layer),
                        None,
                    )?;
                }
            } else {
                model(&live.body, icon, &turned, None, None)?;
            }
        }

        if body_visible
            && self.menu_preview_model.cape_key.is_some()
            && !self.gui_models.wearing_elytra
            && let Some(icon) = self
                .menu_preview_model
                .cape_key
                .as_ref()
                .and_then(|key| self.menu_artwork_icon(key))
            && live.cape.geometry.is_some()
        {
            model(&live.cape, icon, &|_| false, None, None)?;
        }
        for item in self
            .menu_preview_model
            .cosmetics
            .iter()
            .filter(|_| body_visible)
        {
            if let Some(model_source) = live
                .hats
                .get(item)
                .filter(|source| source.geometry.is_some())
                && let Some(icon) = model_source
                    .texture_key
                    .as_deref()
                    .and_then(|key| self.menu_artwork_icon(key))
            {
                model(
                    model_source,
                    icon,
                    &|_| model_source.follows_head,
                    None,
                    None,
                )?;
            }
        }
        // Controller geometry keeps the player's projection; expanded bounds affect coverage,
        // never the model scale or the control's ancestor clip.
        let extra_layers = if in_hud
            && matches!(
                view,
                PreviewView::Doll { .. } | PreviewView::DollLook { .. }
            ) {
            controllers.layers.as_slice()
        } else {
            &[]
        };
        for (source, layer, icon) in extra_layers {
            if source.geometry.is_some() {
                model(source, *icon, &|_| false, Some(layer), None)?;
            }
        }
        if ui_context && !controllers.visibility.hides_held_items() {
            for (hand, layers) in self.gui_models.live_equipment.hands.iter().enumerate() {
                for draw in layers.iter().flatten() {
                    model(
                        &draw.model,
                        draw.icon,
                        &|_| false,
                        Some(&draw.layer),
                        Some(hand),
                    )?;
                }
            }
        }
        if body_visible
            && let Some(model) = self.preview_head_block()
            && let Some(parent) = live.head_bone
        {
            let bone = render_model::equipment::attach_to_bone(
                parent,
                render_model::equipment::head_block_display(),
            )?;
            let frame = live.frame * super::super::live_player::render_matrix(&bone)?;
            // The block turns with the head when a screen's renderer looks toward the pointer.
            let frame = turn.map_or(frame, |turn| turn * frame);
            let (screen, light) = projection(&rig, 6);
            cache.layer(
                &model.vertices,
                std::iter::once(screen * Mat4::from(frame)),
                std::iter::once(light * Mat3::from(frame.matrix3)),
                model.source,
                None,
                live.overlay_color,
            )?;
        }
        for slot in 0..4 {
            let Some(texture) = &self.player_preview_gear.armor[slot] else {
                continue;
            };
            let icon = self
                .gui_models
                .trim_icons
                .region(texture)
                .or_else(|| self.gui_models.pack_equipment.region(slot))
                .or_else(|| {
                    self.gui_models
                        .sources
                        .textures
                        .get(&super::super::atlas::key(
                            [texture.width, texture.height],
                            &texture.rgba,
                        ))
                        .copied()
                });
            let Some(icon) = icon else { continue };
            let frames = [0, 1, 2, 3, 4, 5].map(|part| projection(&rig, part));
            cache.layer(
                &armor_source(slot),
                frames.iter().map(|frame| frame.0),
                frames.iter().map(|frame| normal_frame(frame.1, false)),
                icon.with_glint(self.player_preview_gear.armor_glint[slot]),
                texture.tint,
                live.overlay_color,
            )?;
        }
        for (hand, model) in held.iter().enumerate() {
            if ui_context && self.gui_models.live_equipment.hands[hand].is_some() {
                continue;
            }
            let Some(model) = model else { continue };
            let (frame, part) = if ui_context {
                let Some(parent) = live.hands[hand] else {
                    continue;
                };
                let scale = actor_animation::AttachableAnimationInput {
                    off_hand: hand == 1,
                    held_item_scale: controllers.held_item_scale,
                    ..Default::default()
                }
                .main_hand_scale();
                let (bone, pivot) =
                    player_preview::geometry::held::placement_on_bone(model, hand, parent, scale)?;
                (
                    live.frame
                        * super::super::live_player::render_matrix(&bone)?
                        * Affine3A::from_translation(-pivot),
                    6,
                )
            } else {
                let (bone, pivot) = player_preview::geometry::held::placement(model, hand)?;
                (
                    Affine3A::from_scale(Vec3::new(-1., 1., -1.))
                        * super::super::live_player::render_matrix(&bone)?
                        * Affine3A::from_translation(-pivot),
                    if hand == 0 { 2 } else { 3 },
                )
            };
            let (screen, light) = projection(&rig, part);
            cache.layer(
                &model.vertices,
                std::iter::once(screen * Mat4::from(frame)),
                std::iter::once(light * Mat3::from(frame.matrix3)),
                model.source,
                None,
                [0.; 4],
            )?;
            let index = cache.cursor - 1;
            let cutoff = model.alpha_cutoff();
            let changed = cache.sources[index].material.3 != cutoff;
            cache.sources[index].material.3 = cutoff;
            cache.dirty |= changed;
            cache.sources[index].hand = Some(hand);
        }
        if let Some(fire) = self.gui_fire_frame(view) {
            let mut vertices = std::mem::take(&mut cache.fire_vertices);
            let mut batches = std::mem::take(&mut cache.fire_batches);
            vertices.clear();
            batches.clear();
            let prepared =
                player_preview::geometry::fire::append(&mut vertices, &mut batches, fire).and_then(
                    |()| {
                        batches
                            .iter()
                            .try_for_each(|batch| cache.effect(&vertices, None, batch))
                    },
                );
            cache.fire_vertices = vertices;
            cache.fire_batches = batches;
            prepared?;
        }
        // The particle mesh follows the preview view it was generated for.
        if view == self.player_preview_view
            && let Some(particles) = self.gui_models.particles.mesh.as_deref()
        {
            for batch in particles.batches() {
                cache.effect(particles.vertices(), Some(particles.indices()), batch)?;
            }
        }
        cache.finish(admit)
    }
}

impl HudModel {
    /// Applies the controller's material and texture transforms to the just-appended geometry.
    fn style_last_layer(&mut self, start: usize, layer: &actor_animation::RenderTextureLayer) {
        let state = layer.material_state.unwrap_or_default();
        for bone in &mut self.bones[start..] {
            bone.set_backface_culling(state.cull);
            bone.uv_transform = layer.uv_anim;
            bone.styles[1] = 1;
            bone.tint = layer.color;
            for channel in &mut bone.tint[..3] {
                *channel *= layer.light_color_multiplier;
            }
        }
        let material = (
            ui::UiBlendMode::Alpha,
            !state.depth_always,
            state.depth_write,
            state
                .alpha_test
                .then_some(assets::ENTITY_ALPHA_TEST_THRESHOLD),
        );
        let index = self.cursor - 1;
        self.dirty |= self.sources[index].material != material;
        self.sources[index].material = material;
    }
}
