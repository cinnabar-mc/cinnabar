//! Data-driven held attachables, sharing the entity Molang/animation/controller pipeline.

use actor_animation::{
    ActorRigSnapshot, AttachableAnimationInput,
    attachment::{compose_parent, parent_for_root},
};
use client_world::ActorSnapshot;

use super::*;

/// Suppresses authored hand geometry only when it comes from a server pack.
fn should_fallback_to_icon(show_custom_item_models: bool, from_pack: bool) -> bool {
    !show_custom_item_models && from_pack
}

/// A first-person definition admitted once for both hand placement and model drawing.
pub(crate) struct HeldSelection {
    source: Option<pack::BindingSource>,
    icon_fallback: bool,
}

impl HeldSelection {
    /// Reports whether this admitted definition supplies its own hand placement.
    pub(crate) fn uses_authored_hand(&self, item: &WornItem) -> bool {
        !self.icon_fallback
            && self.source.as_ref().is_some_and(|source| {
                java::keep_authored_hand(
                    &item.identifier,
                    source.catalog.bindings()[source.index].category,
                    source.from_pack,
                )
            })
    }

    /// Whether the selected server-pack model must fall back to the ordinary item sprite.
    pub(crate) const fn uses_icon_fallback(&self) -> bool {
        self.icon_fallback
    }
}

impl EquipmentRuntime {
    /// Admits the first-person definition before choosing its hand and drawing its model; `java`
    /// is Java 1.7 animation mode.
    pub(crate) fn select_held_item(
        &mut self,
        item: &WornItem,
        owner: &ActorSnapshot,
        rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        java: bool,
    ) -> HeldSelection {
        let source = if self.uses_native_item_mesh(&item.identifier, java) {
            None
        } else {
            self.selected_binding_source(
                item,
                EquipmentAnimation {
                    owner,
                    rig,
                    frame_alpha: input.frame_alpha,
                    delta_seconds: input.delta_seconds.unwrap_or_default(),
                },
                input,
            )
        };
        HeldSelection {
            icon_fallback: source.as_ref().is_some_and(|source| {
                should_fallback_to_icon(self.show_custom_item_models, source.from_pack)
            }),
            source,
        }
    }

    /// Draws the owner's admitted attachable, retaining authored hand poses when it owns them.
    pub fn first_person_attachable(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        owner: &ActorSnapshot,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        java_hand: Option<render_model::java_animation::JavaHand>,
    ) -> Option<FirstPersonItem> {
        let selected = self.select_held_item(item, owner, owner_rig, input, java_hand.is_some());
        self.first_person_selected_attachable(
            body, item, &selected, owner, owner_rig, input, java_hand,
        )
    }

    /// Draws the definition already used to choose the first-person hand placement.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn first_person_selected_attachable(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        selected: &HeldSelection,
        owner: &ActorSnapshot,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        java_hand: Option<render_model::java_animation::JavaHand>,
    ) -> Option<FirstPersonItem> {
        if selected.uses_icon_fallback() {
            return None;
        }
        self.held_attachable_from_source(
            body,
            item,
            selected.source.as_ref()?,
            owner,
            owner_rig,
            input,
            java_hand,
        )
    }

    /// Samples an already admitted held definition without selecting another fallback.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn held_attachable_from_source(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        source: &pack::BindingSource,
        owner: &ActorSnapshot,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        java_hand: Option<render_model::java_animation::JavaHand>,
    ) -> Option<FirstPersonItem> {
        let (_, body_bones) = self.body_bones_for(body.input.rig)?;
        let pack::BindingSource {
            ref catalog,
            from_pack,
            index,
            ..
        } = *source;
        let binding = &catalog.bindings()[index];
        let java_hand = java_hand.filter(|_| {
            !super::java::keep_authored_hand(&item.identifier, binding.category, from_pack)
        });
        if !matches!(
            self.effective_category(&item.identifier, binding.category),
            EquipmentCategory::Held
                | EquipmentCategory::ArmorAndHeld { .. }
                | EquipmentCategory::Shield
        ) {
            return None;
        }
        let (assets, runtime) = if from_pack {
            let pack = self.pack.as_mut()?;
            (Arc::clone(&pack.assets), &mut pack.attachables)
        } else {
            (Arc::clone(&self.assets), &mut self.attachables)
        };
        let evaluated =
            runtime.evaluate(binding.attachable_identifier(), owner, owner_rig, input)?;
        // Each hand has its own item texture page. Multi-layer attachables remain
        // explicitly incomplete rather than accidentally displaying an enchantment layer.
        let selected = evaluated
            .render
            .iter()
            .find(|layer| layer.texture_slot == 0)?;
        let style = super::style::LayerStyle::from_layer(selected);
        let geometry_index = selected.geometry.unwrap_or(evaluated.geometry);
        let pose = if selected.pose.is_empty() {
            evaluated.pose
        } else {
            &selected.pose
        };
        let held_scale = AttachableAnimationInput {
            held_item_scale: owner_rig.held_item_scale,
            ..input
        }
        .main_hand_scale();
        let model_scale = std::array::from_fn::<_, 3, _>(|axis| {
            evaluated.scale * evaluated.axis_scale[axis] * held_scale
        });
        let placed = self.attachment_poses.sample(
            body,
            (
                false,
                if input.off_hand {
                    LAYER_OFF_HAND
                } else {
                    LAYER_MAIN_HAND
                },
            ),
            self.poses.frame,
            placement_cache::Inputs {
                assets: &assets,
                bones: &body_bones,
                owner_names: owner_rig.bone_names,
                geometry: (geometry_index, evaluated.geometry),
                local: pose,
                paired: !input.first_person,
                hidden: &selected.hidden_bones,
                scale: model_scale,
                alpha: input
                    .first_person
                    .then(|| {
                        placement_cache::alpha(
                            body,
                            &evaluated,
                            geometry_index,
                            pose.len(),
                            input.frame_alpha,
                        )
                    })
                    .flatten(),
            },
            |endpoint, index| {
                if selected.hidden_bones.contains(&(index as u32)) {
                    return Some(hidden_bone());
                }
                compose_parent(
                    parent_for_root(
                        &body.input.previous_bones,
                        &body.input.current_bones,
                        &body_bones.names,
                        if input.first_person {
                            input
                        } else {
                            AttachableAnimationInput {
                                frame_alpha: endpoint as f32,
                                ..input
                            }
                        },
                        evaluated.bone_parent(geometry_index, index)?,
                        model_scale,
                    )?,
                    pose[index],
                )
            },
        )?;
        let texture_source = assets.sources().get(selected.source as usize)?;
        let texture_identifier = texture_source
            .path
            .strip_suffix(".png")
            .or_else(|| texture_source.path.strip_suffix(".tga"))
            .unwrap_or(&texture_source.path);
        let (artwork, texture_index, location) = self.draw_artwork(texture_identifier, source)?;
        let texture = &artwork.textures()[texture_index];
        let key = (
            from_pack,
            geometry_index,
            (location.page(), location.layer()),
        );
        // Java draws a raster attachable (the bow's pull frames) as its own flat item.
        let java = java_hand
            .filter(|_| super::java::java_draws_attachable(&item.identifier))
            .and_then(|hand| {
                let raster = self
                    .java_rasters
                    .entry(key)
                    .or_insert_with(|| {
                        let (image_to_rig, pivots) = render_model::attachable_raster_frame(
                            &assets,
                            geometry_index as usize,
                            texture,
                        )?;
                        Some(JavaRasterFrame {
                            image_to_rig,
                            rest: pivots.into_iter().map(super::java::rest_bone).collect(),
                            normal_axis: image_to_rig
                                .transform_vector3(-bevy::math::Vec3::Y)
                                .normalize(),
                        })
                    })
                    .clone()?;
                let camera = super::java::java_raster_camera(
                    hand,
                    raster.image_to_rig,
                    texture.width,
                    texture.height,
                    held_scale,
                );
                camera
                    .is_finite()
                    .then_some((camera, raster.rest, raster.normal_axis))
            });
        let (java_camera, placed, java_normal_axis) = match java {
            Some((camera, rest, normal_axis)) => {
                (Some(camera), [Arc::clone(&rest), rest], normal_axis)
            }
            None => (None, placed, bevy::math::Vec3::Z),
        };
        let rig = if let Some(rig) = self.attachable_meshes.get(&key) {
            *rig
        } else {
            let rig = self.build_item_mesh(|rig| {
                render_model::attachable_geometry(&assets, geometry_index as usize, rig, texture)
                    .ok()
            })?;
            self.attachable_meshes.insert(key, rig);
            rig
        };
        let mut presentation = layer_presentation(
            body,
            if input.off_hand {
                LAYER_OFF_HAND
            } else {
                LAYER_MAIN_HAND
            },
            rig,
            placed,
            location,
            0,
        );
        style.apply_held(&mut presentation.submission);
        Some(FirstPersonItem {
            presentation: super::push::attachable_glint(presentation, item),
            camera_space: java_camera.is_some(),
            alpha_mode: render_model::HandItemAlphaMode::Cutout,
            emitted_light: 0,
            java_camera,
            java_normal_axis,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_item_model_setting_downgrades_only_server_pack_models() {
        assert!(should_fallback_to_icon(false, true));
        assert!(!should_fallback_to_icon(true, true));
        assert!(!should_fallback_to_icon(false, false));
    }
}
