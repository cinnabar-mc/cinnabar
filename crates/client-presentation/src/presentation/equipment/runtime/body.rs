//! Body-slot attachables, and attachables that name no worn slot, retain their authored
//! owner binding and controller output.

use super::*;

impl EquipmentRuntime {
    /// Draws the body slot with the same owner context and root placement as other attachables.
    pub(super) fn push_body(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        equipment: &ActorEquipmentInput,
        animation: EquipmentAnimation<'_>,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let variables = [(
            assets::entity::EngineVariable::IsEnchanted.name(),
            f32::from(item.enchanted),
        )];
        let input = actor_animation::AttachableAnimationInput {
            worn_slot: actor_animation::AttachableAnimationInput::BODY_SLOT,
            ..animation.worn_input(equipment, ArmorSlot::Helmet, &variables)
        };
        let Some(source) = self.selected_binding_source(item, animation, input) else {
            return;
        };
        self.push_worn_attachable(
            body,
            &source,
            input,
            animation,
            super::super::BODY_EQUIPMENT_LAYER,
            layers,
        );
    }

    /// Draws an attachable whose item names no worn slot in the armor slot that holds it, as
    /// native does for a server cosmetic. Its own controllers see that slot.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_slot_attachable(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        source: &pack::BindingSource,
        equipment: &ActorEquipmentInput,
        animation: EquipmentAnimation<'_>,
        (slot, layer): (ArmorSlot, u8),
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let variables = [(
            assets::entity::EngineVariable::IsEnchanted.name(),
            f32::from(item.has_glint()),
        )];
        let input = animation.worn_input(equipment, slot, &variables);
        self.push_worn_attachable(body, source, input, animation, layer, layers);
    }

    /// Draws one worn attachable on `layer`: its controllers' geometry, texture and authored
    /// pose, with each root bound to its owner bone.
    fn push_worn_attachable(
        &mut self,
        body: &ActorRigSubmission,
        source: &pack::BindingSource,
        input: actor_animation::AttachableAnimationInput<'_>,
        animation: EquipmentAnimation<'_>,
        layer: u8,
        layers: &mut Vec<EquipmentPresentation>,
    ) {
        let Some((_, bones)) = self.body_bones_for(body.input.rig) else {
            return;
        };
        let binding = &source.catalog.bindings()[source.index];
        let (assets, runtime) = if source.from_pack {
            let Some(pack) = self.pack.as_mut() else {
                return;
            };
            (Arc::clone(&pack.assets), &mut pack.attachables)
        } else {
            (Arc::clone(&self.assets), &mut self.attachables)
        };
        let registered = (binding.geometry.resolution == EntityDependencyResolution::Catalog)
            .then(|| find_geometry_index(&assets, &binding.geometry.identifier))
            .flatten();
        let Some(evaluated) = runtime.evaluate(
            binding.attachable_identifier(),
            animation.owner,
            animation.rig,
            input,
        ) else {
            return;
        };
        let Some(selected) = evaluated
            .render
            .iter()
            .find(|layer| layer.texture_slot == 0)
        else {
            return;
        };
        let geometry_index = selected.geometry.unwrap_or(evaluated.geometry);
        let local = if selected.pose.is_empty() {
            evaluated.pose
        } else {
            &selected.pose
        };
        let scale = evaluated.axis_scale.map(|axis| axis * evaluated.scale);
        let Some(poses) = self.attachment_poses.sample(
            body,
            (true, input.worn_slot),
            self.poses.frame,
            placement_cache::Inputs {
                assets: &assets,
                bones: &bones,
                owner_names: animation.rig.bone_names,
                owner_layers: animation.rig.render,
                geometry: (geometry_index, evaluated.geometry),
                local,
                paired: false,
                hidden: &selected.hidden_bones,
                scale,
                alpha: placement_cache::alpha(
                    body,
                    animation.rig.render,
                    &evaluated,
                    geometry_index,
                    local.len(),
                    input.frame_alpha,
                ),
            },
            |_, index| {
                if selected.hidden_bones.contains(&(index as u32)) {
                    return Some(hidden_bone());
                }
                actor_animation::attachment::compose_parent(
                    actor_animation::attachment::parent_for_root(
                        actor_animation::attachment::OwnerSkeleton::body(
                            &body.input.previous_bones,
                            &body.input.current_bones,
                            &bones.names,
                        )
                        .with_layers(animation.rig),
                        input,
                        evaluated.bone_parent(geometry_index, index)?,
                        scale,
                    )?,
                    local[index],
                )
            },
        ) else {
            return;
        };
        let style = style::LayerStyle::from_layer(selected);
        let Some(texture_source) = assets.sources().get(selected.source as usize) else {
            return;
        };
        let texture = texture_source
            .path
            .strip_suffix(".png")
            .or_else(|| texture_source.path.strip_suffix(".tga"))
            .unwrap_or(&texture_source.path);
        let Some((artwork, texture_index, location)) = self.draw_artwork(texture, source) else {
            return;
        };
        let rig = if self.has_texture_meshes(&assets, source.from_pack, geometry_index) {
            // Texture meshes are extruded from the selected texture's pixels.
            let key = (
                source.from_pack,
                geometry_index,
                (location.page(), location.layer()),
            );
            let texture = &artwork.textures()[texture_index];
            let Some(rig) = self.attachable_mesh(&assets, key, texture) else {
                return;
            };
            rig
        } else {
            let Some(geometry) = assets.geometries().get(geometry_index as usize) else {
                return;
            };
            let Some(geometry) = self.armor_geometry_for(&geometry.identifier, source.from_pack)
            else {
                return;
            };
            let registered = if source.from_pack {
                registered == Some(geometry_index)
            } else {
                self.startup_geometries.contains(&geometry.rig)
            };
            if !registered && !self.selected_geometries.contains(&geometry.rig) {
                let Some(mesh) = render_model::equipment_geometry(
                    &assets,
                    geometry_index as usize,
                    geometry.rig,
                ) else {
                    return;
                };
                self.pending.push(mesh);
                self.selected_geometries.insert(geometry.rig);
            }
            geometry.rig
        };
        let mut presentation = layer_presentation(body, layer, rig, poses, location, 0);
        style.apply(&mut presentation.submission);
        layers.push(presentation);
    }

    /// Whether geometry `geometry_index` has texture meshes, resolved once per installed pack.
    fn has_texture_meshes(
        &mut self,
        assets: &assets::RuntimeEntityAssets,
        from_pack: bool,
        geometry_index: u32,
    ) -> bool {
        *self
            .texture_mesh_geometries
            .entry((from_pack, geometry_index))
            .or_insert_with(|| {
                render_model::resolve_geometry_bones(assets, geometry_index as usize)
                    .is_ok_and(|bones| bones.iter().any(|bone| !bone.texture_meshes.is_empty()))
            })
    }
}
