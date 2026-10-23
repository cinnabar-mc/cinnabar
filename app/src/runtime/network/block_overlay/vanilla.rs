//! State-aware material replacement and block-specific texture rebinding.

use super::*;

impl Builder<'_> {
    /// Replaces selectors only for new catalog entries; raster-only packs replace their leaves.
    pub(super) fn override_vanilla_materials(&mut self, keys: &MaterialKeys) {
        let mut routes = keys
            .keys()
            .flat_map(|key| keys.materials(key).iter().map(move |&id| (id, key)))
            .chain(
                keys.material_sources()
                    .map(|(id, source)| (id, source.key.as_ref())),
            )
            .collect::<Vec<_>>();
        routes.sort_unstable();
        routes.dedup();
        let mut claimed = HashSet::new();
        for (id, key) in routes {
            if id == DIAGNOSTIC_MATERIAL || claimed.contains(&id) {
                continue;
            }
            let source = keys.material_source(id);
            let variant = source.map_or(keys.flipbook_selector(id).0, |source| source.variant);
            let base = self
                .base_assets
                .and_then(|base| base.materials().get(id as usize))
                .copied();
            if self.catalog.redefined(key, variant) {
                // Weighted leaves become unused when their selector is replaced.
                if base.is_some_and(|material| material.variation_weight != 0) {
                    continue;
                }
                let flags = base.map_or(0, |material| material.flags);
                if let Some(replacement) = self.terrain_material(keys, key, variant, flags) {
                    let replacement = self.overlay.materials[replacement as usize];
                    claimed.insert(id);
                    self.overlay.material_overrides.push(MaterialOverride {
                        material: id,
                        texture: replacement.texture,
                        animation: replacement.animation,
                        variation: Some((replacement.variation_start, replacement.variation_count)),
                    });
                }
            } else {
                let route = source.map_or_else(
                    || TextureRoute::Catalog {
                        key: key.to_owned(),
                        variant,
                        choice: keys.flipbook_selector(id).1,
                    },
                    |source| TextureRoute::Path {
                        variant: source.variant,
                        key: key.to_owned(),
                        path: source.path.to_string(),
                        choice: keys.flipbook_selector(id).1,
                    },
                );
                if let Some(slot) = self.route_slot(route) {
                    claimed.insert(id);
                    self.overlay.material_overrides.push(MaterialOverride {
                        material: id,
                        texture: TextureRef::new(slot.page, slot.layer).expect("bounded layer"),
                        animation: slot.animation,
                        variation: None,
                    });
                }
            }
        }
    }

    /// Rebinds faces per visual so blocks that share a material stay independent.
    pub(super) fn override_vanilla_bindings(
        &mut self,
        keys: &MaterialKeys,
        bindings: &legacy::TextureBindings,
    ) {
        let Some(base) = self.base_assets else {
            return;
        };
        let overridden = self
            .overlay
            .material_overrides
            .iter()
            .map(|replacement| replacement.material)
            .collect::<HashSet<_>>();
        for source in keys.block_sources() {
            let block = base.resolve(assets::NetworkIdMode::Sequential, source.visual);
            if !source.cube_binding_eligible
                || !block.is_known()
                || block.kind() != VisualKind::Cube
                || block.model_template().is_some()
                || block.variant() != 0
                || block.flags().contains(assets::BlockFlags::LEAF_MODEL)
                || assets::BlockFace::ALL.into_iter().any(|face| {
                    base.material(block.face(face).material_id()).flags
                        & assets::MATERIAL_FLAG_TINT_MASK
                        != 0
                })
            {
                continue;
            }
            let world_bindings = assets::BlockFace::ALL.map(|face| {
                let index = face as usize;
                let id = block.face(face).material_id();
                let material = base.material(id);
                let binding = bindings.binding(
                    &source.name,
                    &source.binding_name,
                    source.source_faces[index] as usize,
                    false,
                )?;
                let rotation = if binding.per_face && source.source_rotations[index] {
                    assets::MATERIAL_FLAG_ROTATE_UV
                } else {
                    0
                };
                let flags = (material.flags & !assets::MATERIAL_FLAG_ROTATE_UV) | rotation;
                (flags != material.flags
                    || keys
                        .material_source(id)
                        .is_none_or(|original| original.key.as_ref() != binding.key))
                .then_some((binding.key, flags))
            });
            let carried_keys = assets::BlockFace::ALL.map(|face| {
                let index = face as usize;
                let material = block.face(face).material_id();
                bindings
                    .texture(&source.name, &source.binding_name, index, true)
                    .or(source.carried_keys[index].as_deref())
                    .or_else(|| bindings.texture(&source.name, &source.binding_name, index, false))
                    .or(source.carried_fallback_keys[index].as_deref())
                    .or_else(|| {
                        keys.material_source(material)
                            .map(|source| source.key.as_ref())
                    })
            });
            let affected = assets::BlockFace::ALL.into_iter().any(|face| {
                let index = face as usize;
                let id = block.face(face).material_id();
                let material = base.material(id);
                let base_carried = source.carried_keys[index]
                    .as_deref()
                    .or(source.carried_fallback_keys[index].as_deref())
                    .or_else(|| keys.material_source(id).map(|source| source.key.as_ref()));
                overridden.contains(&block.face(face).material_id())
                    || (material.variation_start
                        ..material.variation_start + material.variation_count)
                        .any(|id| overridden.contains(&id))
                    || world_bindings[index].is_some()
                    || carried_keys[index] != base_carried
            });
            let carried_changed = !affected
                && assets::BlockFace::ALL.into_iter().any(|face| {
                    let index = face as usize;
                    let variant = keys
                        .material_source(block.face(face).material_id())
                        .map_or(0, |source| source.variant);
                    carried_keys[index]
                        .into_iter()
                        .any(|key| self.carried_changed(key, variant))
                });
            if !affected && !carried_changed {
                continue;
            }
            if carried_keys.iter().enumerate().any(|(index, key)| {
                let material = block.face(assets::BlockFace::ALL[index]).material_id();
                let variant = keys
                    .material_source(material)
                    .map_or(0, |source| source.variant);
                key.and_then(|key| self.catalog.choices(key, variant))
                    .is_some_and(|choices| {
                        choices
                            .iter()
                            .any(|choice| choice.tint.is_some() || choice.overlay_color)
                    })
            }) {
                continue;
            }
            let mut faces = [None; 6];

            for face in assets::BlockFace::ALL {
                let index = face as usize;
                let material = block.face(face).material_id();
                let variant = keys
                    .material_source(material)
                    .map_or(0, |source| source.variant);
                if let Some((key, flags)) = world_bindings[index] {
                    faces[index] = self.terrain_material(keys, key, variant, flags);
                }
            }
            if faces.iter().any(Option::is_some) {
                self.overlay.face_overrides.push((source.visual, faces));
            }
            let checkpoint = self.texture_checkpoint();
            let carried = assets::BlockFace::ALL.map(|face| {
                let index = face as usize;
                let material = block.face(face).material_id();
                let variant = keys
                    .material_source(material)
                    .map_or(0, |source| source.variant);
                let flags = base.material(material).flags & !assets::MATERIAL_FLAG_ROTATE_UV;
                self.terrain_material(keys, carried_keys[index]?, variant, flags)
            });
            if carried.iter().all(Option::is_some) {
                self.vanilla_carried.push(CarriedBlock {
                    visual: source.visual,
                    faces: carried.map(Option::unwrap),
                });
            } else {
                self.rollback_textures(checkpoint);
            }
        }
    }

    /// Tests optional carried routes without copying unrelated carrier pixels into the overlay.
    fn carried_changed(&mut self, key: &str, variant: u32) -> bool {
        if self.catalog.redefined(key, variant) {
            return true;
        }
        let count = self
            .catalog
            .choices(key, variant)
            .map_or(0, |choices| choices.len());
        let checkpoint = self.texture_checkpoint();
        let changed = (0..count).any(|choice| {
            self.route_slot(TextureRoute::Catalog {
                key: key.to_owned(),
                variant,
                choice: choice as u32,
            })
            .is_some()
        });
        self.rollback_textures(checkpoint);
        changed
    }

    /// Builds one state selector with independent, normalized positional alternatives.
    pub(super) fn terrain_material(
        &mut self,
        keys: &MaterialKeys,
        key: &str,
        variant: u32,
        flags: u32,
    ) -> Option<u32> {
        let variant = self.catalog.resolved_variant(key, variant)?;
        let cache_key = (key.to_owned(), variant, flags);
        if let Some(&id) = self.terrain_materials.get(&cache_key) {
            return Some(id);
        }
        let choices = self.catalog.choices(key, variant)?.to_vec();
        let added = choices.len() + usize::from(choices.len() > 1);
        if !self.can_admit_materials(added) {
            return None;
        }
        let checkpoint = self.texture_checkpoint();
        let leaves = choices
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                let slot = self.carrier_slot(
                    TextureRoute::Catalog {
                        key: key.to_owned(),
                        variant,
                        choice: index as u32,
                    },
                    keys,
                )?;
                Some(Material {
                    texture: TextureRef::new(slot.page, slot.layer).expect("bounded layer"),
                    flags,
                    animation: slot.animation,
                    variation_weight: if choices.len() == 1 {
                        0
                    } else {
                        choice.weight.to_bits()
                    },
                    ..Material::unvaried()
                })
            })
            .collect::<Option<Vec<_>>>();
        let Some(leaves) = leaves else {
            self.rollback_textures(checkpoint);
            return None;
        };
        let start = self.overlay.materials.len() as u32;
        let mut selector = leaves[0];
        self.overlay.materials.extend(leaves);
        let id = if choices.len() > 1 {
            selector.variation_start = start;
            selector.variation_count = choices.len() as u32;
            selector.variation_weight = 0;
            let id = self.overlay.materials.len() as u32;
            self.overlay.materials.push(selector);
            id
        } else {
            start
        };
        self.terrain_materials.insert(cache_key, id);
        Some(id)
    }
}
