//! Data-driven held attachables, sharing the entity Molang/animation/controller pipeline.

use actor_animation::{
    ActorRigSnapshot, AttachableAnimationInput,
    attachment::{OwnerSkeleton, compose_parent, parent_for_root},
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
    owns_definition: bool,
}

impl HeldSelection {
    /// Whether the selected binding owns an authored definition that can be evaluated,
    /// independently of whether that definition drew. A geometry-only startup fallback
    /// is admitted for missing artwork but has no definition to occupy the hand's draw slot.
    pub(crate) const fn admitted(&self) -> bool {
        self.owns_definition
    }

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
        let owns_definition = source.as_ref().is_some_and(|source| {
            let assets = if source.from_pack {
                self.pack.as_ref().map(|pack| &pack.assets)
            } else {
                Some(&self.assets)
            };
            assets
                .and_then(|assets| {
                    assets.attachable_rig_binding(
                        source.catalog.bindings()[source.index].attachable_identifier(),
                    )
                })
                .is_some()
        });
        HeldSelection {
            icon_fallback: source.as_ref().is_some_and(|source| {
                should_fallback_to_icon(self.show_custom_item_models, source.from_pack)
            }),
            owns_definition,
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
        let mut hidden = false;
        self.held_attachable_from_source_with_policy(
            body,
            item,
            source,
            owner,
            owner_rig,
            input,
            java_hand,
            false,
            &mut hidden,
        )
    }

    /// Evaluates and renders one admitted held definition, optionally consuming an exact world
    /// main-hand authored-zero proof from that same evaluation.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn held_attachable_from_source_with_policy(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        source: &pack::BindingSource,
        owner: &ActorSnapshot,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        java_hand: Option<render_model::java_animation::JavaHand>,
        allow_world_hide: bool,
        hidden: &mut bool,
    ) -> Option<FirstPersonItem> {
        // First person samples only the hand's body pose, not its controller models, so roots
        // bind to and follow the body alone there.
        let owner_rig = &ActorRigSnapshot {
            render: if input.first_person {
                &[]
            } else {
                owner_rig.render
            },
            ..*owner_rig
        };
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
        let eligible_world_main_hand = allow_world_hide
            && self.show_custom_item_models
            && binding.identifier.as_ref() == item.identifier.as_ref()
            && java_hand.is_none()
            && !input.first_person
            && !input.off_hand
            && !input.worn
            && !input.is_paperdoll
            && !input.is_in_ui
            && input.owner_main_hand == Some(item.identifier.as_ref());
        let evaluated = if eligible_world_main_hand {
            match runtime.evaluate_world_main_hand(
                binding.attachable_identifier(),
                owner,
                owner_rig,
                input,
            ) {
                actor_animation::WorldAttachableEvaluation::Drawable(snapshot) => Some(snapshot),
                actor_animation::WorldAttachableEvaluation::AuthoredZeroRootSample(sample) => {
                    match authored_zero_sample_decision(
                        sample.drawable,
                        Self::authored_zero_root_has_valid_world_endpoints(
                            &sample.proof,
                            binding.attachable_identifier(),
                            owner_rig.actor,
                            body,
                            &body_bones,
                            owner_rig,
                            input,
                        ),
                    ) {
                        AuthoredZeroSampleDecision::Hide => {
                            *hidden = true;
                            return None;
                        }
                        AuthoredZeroSampleDecision::Draw(snapshot) => Some(snapshot),
                        AuthoredZeroSampleDecision::Fallback => None,
                    }
                }
                actor_animation::WorldAttachableEvaluation::Unavailable => None,
            }
        } else {
            runtime.evaluate(binding.attachable_identifier(), owner, owner_rig, input)
        }?;
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
                owner_layers: owner_rig.render,
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
                            owner_rig.render,
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
                        OwnerSkeleton::body(
                            &body.input.previous_bones,
                            &body.input.current_bones,
                            &body_bones.names,
                        )
                        .with_layers(owner_rig),
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
        let rig = self.attachable_mesh(&assets, key, texture)?;
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

fn bone_transform_is_finite(transform: actor_animation::BoneTransform) -> bool {
    transform.rotation.iter().all(|value| value.is_finite())
        && transform
            .translation_scale
            .iter()
            .all(|value| value.is_finite())
        && transform.axis_scale.iter().all(|value| value.is_finite())
}

fn valid_composed_zero_root_endpoints(
    mut compose: impl FnMut(f32) -> Option<RenderBoneTransform>,
) -> bool {
    [0.0, 1.0].into_iter().all(|endpoint| {
        compose(endpoint).is_some_and(|placed| {
            placed.is_finite() && placed.axis_scale[..3].iter().all(|scale| *scale == 0.0)
        })
    })
}

impl EquipmentRuntime {
    /// Requires the retained local evidence to compose as a real finite zero-scale pose at both
    /// body endpoints. No renderer sentinel or missing owner binding is accepted as placement.
    fn authored_zero_root_has_valid_world_endpoints(
        proof: &actor_animation::AuthoredZeroRootProof<'_>,
        identifier: &str,
        owner: actor_animation::ActorLifetimeId,
        body: &ActorRigSubmission,
        body_bones: &BodyBones,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
    ) -> bool {
        if proof.attachable_identifier != identifier
            || proof.owner != owner
            || proof.effective_scale != [0.0; 3]
            || proof.rig_scale.iter().any(|value| !value.is_finite())
            || !bone_transform_is_finite(proof.root_transform)
        {
            return false;
        }
        let placement_input = AttachableAnimationInput {
            held_item_scale: owner_rig.held_item_scale,
            ..input
        };
        let held_scale = placement_input.main_hand_scale();
        let model_scale =
            std::array::from_fn(|axis| proof.rig_scale[0] * proof.rig_scale[axis + 1] * held_scale);
        if model_scale.iter().any(|value| !value.is_finite()) {
            return false;
        }
        let skeleton = actor_animation::attachment::OwnerSkeleton::body(
            &body.input.previous_bones,
            &body.input.current_bones,
            &body_bones.names,
        )
        .with_layers(owner_rig);
        valid_composed_zero_root_endpoints(|endpoint| {
            let endpoint_input = AttachableAnimationInput {
                frame_alpha: endpoint,
                ..placement_input
            };
            let parent = actor_animation::attachment::parent_for_root(
                skeleton,
                endpoint_input,
                proof.root_parent,
                model_scale,
            )?;
            actor_animation::attachment::compose_parent(parent, proof.root_transform)
        })
    }

    /// The mesh of attachable geometry `key.1` drawn with `texture`, built once per artwork
    /// page. Its texture meshes are extruded from that texture's pixels.
    pub(super) fn attachable_mesh(
        &mut self,
        assets: &assets::RuntimeEntityAssets,
        key: AttachableMeshKey,
        texture: &assets::EquipmentTexture,
    ) -> Option<EntityRigId> {
        if let Some(rig) = self.attachable_meshes.get(&key) {
            return Some(*rig);
        }
        let rig = self.build_item_mesh(|rig| {
            render_model::attachable_geometry(assets, key.1 as usize, rig, texture).ok()
        })?;
        self.attachable_meshes.insert(key, rig);
        Some(rig)
    }
}

enum AuthoredZeroSampleDecision<T> {
    Hide,
    Draw(T),
    Fallback,
}

fn authored_zero_sample_decision<T>(
    drawable: Option<T>,
    proof_accepted: bool,
) -> AuthoredZeroSampleDecision<T> {
    if proof_accepted {
        AuthoredZeroSampleDecision::Hide
    } else if let Some(drawable) = drawable {
        AuthoredZeroSampleDecision::Draw(drawable)
    } else {
        AuthoredZeroSampleDecision::Fallback
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

    #[test]
    fn zero_root_requires_two_finite_composed_placements() {
        let valid = || RenderBoneTransform {
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: [0.0, 0.0, 0.0, 1.0],
        };
        assert!(valid_composed_zero_root_endpoints(|_| Some(valid())));
        assert!(!valid_composed_zero_root_endpoints(|endpoint| {
            (endpoint == 0.0).then(valid)
        }));
        assert!(!valid_composed_zero_root_endpoints(|endpoint| {
            let mut pose = valid();
            if endpoint == 1.0 {
                pose.translation_scale[0] = f32::NAN;
            }
            Some(pose)
        }));
    }

    #[test]
    fn rejected_zero_root_uses_the_same_optional_drawable() {
        let original: Arc<str> = Arc::from("original evaluated drawable");
        match authored_zero_sample_decision(Some(Arc::clone(&original)), false) {
            AuthoredZeroSampleDecision::Draw(drawable) => {
                assert!(Arc::ptr_eq(&drawable, &original));
            }
            AuthoredZeroSampleDecision::Hide | AuthoredZeroSampleDecision::Fallback => {
                panic!("rejected proof must keep its original drawable")
            }
        }
        assert!(matches!(
            authored_zero_sample_decision::<Arc<str>>(None, false),
            AuthoredZeroSampleDecision::Fallback
        ));
        assert!(matches!(
            authored_zero_sample_decision(Some(original), true),
            AuthoredZeroSampleDecision::Hide
        ));
    }
}
