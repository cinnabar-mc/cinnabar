//! Static item meshes and atlas pages prepared before installation on the UI frame.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::{ItemVisualDefinitionRoute, RuntimeAssets, RuntimeEntityAssets};

use super::{
    GuiModelPreparation, MODEL_PAGES, PreparedGuiModels, UiPresentationError, atlas, block_models,
    held, icon_key, item_gui, player_preview, shield_icons,
    sources::{gui_cube_sheet, ordinary_cube_sheet, sheet_faces},
};

/// A sheet can become a cube only when an existing carried model spans the full cube.
/// Shortened models keep the shaped-model path even when their source texels are transparent.
pub(super) fn cube_shape_admitted(
    ordinary: &render_model::equipment::blocks::BlockSheets,
    visual: u32,
) -> bool {
    !ordinary.models.get(&visual).is_some_and(|vertices| {
        vertices.iter().any(|vertex| {
            vertex
                .position
                .iter()
                .any(|coordinate| coordinate.abs() != 0.5)
        })
    })
}

impl GuiModelPreparation {
    /// Builds static item sources against the menu's existing atlas addresses.
    pub fn prepare(
        self,
        world: &RuntimeAssets,
        entities: &RuntimeEntityAssets,
        equipment: Option<&assets::RuntimeEquipmentCatalog>,
    ) -> Result<PreparedGuiModels, UiPresentationError> {
        let Some(icons) = self.icons.as_deref() else {
            return Ok(PreparedGuiModels::default());
        };
        if icons.source_manifest_sha256() != entities.source_manifest_sha256()
            || (!world.is_diagnostic()
                && world.provenance().source_manifest_sha256 != entities.source_manifest_sha256())
        {
            return Err(UiPresentationError::InvalidFontTexture);
        }
        let first = self.first;
        let mut atlas = atlas::Atlas::new(first, MODEL_PAGES);
        let mut sources = BTreeMap::new();
        let ordinary = render_model::equipment::blocks::collect(world, entities);
        for (&visual, &index) in &ordinary.by_visual {
            sources.insert(visual, ordinary.sheets[index].clone());
        }
        // Native carried masks/colors (not the biome-tinted world faces) override ordinary cubes.
        for sheet in icons.block_sheets() {
            sources.insert(
                sheet.visual.0,
                icons.sprites()[sheet.sprite as usize].clone(),
            );
        }
        let unshaded_visuals = entities
            .item_visuals()
            .iter()
            .filter_map(|definition| {
                let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
                    return None;
                };
                (assets::gui_item::cube_lighting(&definition.key.identifier)
                    == assets::gui_item::CubeLighting::Unshaded)
                    .then_some(block_visual.0)
            })
            .collect::<BTreeSet<_>>();
        let lighting_for_visual = |visual| {
            if unshaded_visuals.contains(&visual) {
                assets::gui_item::CubeLighting::Unshaded
            } else {
                assets::gui_item::CubeLighting::Directional
            }
        };
        for (visual, sprite) in &sources {
            if !cube_shape_admitted(&ordinary, *visual) || !ordinary_cube_sheet(&sprite.rgba8) {
                continue;
            }
            atlas.insert([sprite.width, sprite.height], &sprite.rgba8)?;
        }
        let mut models = BTreeMap::new();
        let mut shield = None;
        if let Some(equipment) = equipment {
            for texture in equipment.textures() {
                atlas.insert([texture.width, texture.height], &texture.rgba8)?;
            }
            if let Some(binding) = equipment
                .bindings_for(assets::gui_item::SHIELD_IDENTIFIER)
                .iter()
                .rev()
                .find(|binding| player_preview::selection::unrestricted(binding, Some(entities)))
                && let Some(geometry) = entities
                    .geometries()
                    .iter()
                    .find(|geometry| geometry.identifier == binding.geometry.identifier)
                && let Some(texture) = equipment.texture(&binding.texture.identifier)
                && let Some(icon) = self.shield
            {
                let source = atlas.insert([texture.width, texture.height], &texture.rgba8)?;
                if let Some(mesh) = item_gui::shield(geometry, texture, source) {
                    models.insert(icon_key(icon), mesh);
                    shield = Some(Arc::new(shield_icons::Source {
                        geometry: geometry.clone(),
                        texture: texture.clone(),
                    }));
                }
            }
        }
        let (held, held_texture_fallbacks) = held::prepare(
            &mut atlas,
            entities,
            icons,
            self.icon_refs
                .as_deref()
                .ok_or(UiPresentationError::InvalidFontTexture)?,
            equipment,
            world,
        )?;
        let required_pages = atlas.page_count();
        for sheet in icons.gui_block_sheets() {
            sources.insert(
                sheet.visual.0,
                icons.sprites()[sheet.sprite as usize].clone(),
            );
        }
        let mut meshes = BTreeMap::new();
        for (visual, sprite) in &sources {
            if !cube_shape_admitted(&ordinary, *visual) || !ordinary_cube_sheet(&sprite.rgba8) {
                continue;
            }
            // Shared held artwork stays required; new GUI artwork uses the optional path below.
            if let Some(icon) = atlas.get([sprite.width, sprite.height], &sprite.rgba8)
                && let Some(mesh) = item_gui::cube(sheet_faces(icon), lighting_for_visual(*visual))
            {
                meshes.insert(*visual, (mesh, false));
            }
        }
        let mut optional_models = BTreeSet::new();
        let mut faded_thumbnails = BTreeSet::new();
        let mut gui_thumbnails = BTreeSet::new();
        for entry in icons.entries() {
            let Some(definition) = entities.item_visuals().iter().find(|visual| {
                visual.key.identifier == entry.identifier && visual.key.metadata == entry.metadata
            }) else {
                continue;
            };
            let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
                continue;
            };
            if icons
                .gui_block_sheets()
                .binary_search_by_key(&block_visual.0, |sheet| sheet.visual.0)
                .is_ok()
            {
                gui_thumbnails.insert(entry.sprite);
            }
            let Some(icon) = self
                .icon_refs
                .as_deref()
                .and_then(|refs| refs.get(entry.sprite as usize))
            else {
                continue;
            };
            let key = icon_key(*icon);
            if models.contains_key(&key) {
                continue;
            }
            // Native cube meshes bake directional lighting; preserve unshaded thumbnails.
            let block = world.resolve(assets::NetworkIdMode::Sequential, block_visual.0);
            if assets::BlockFace::ALL.into_iter().any(|face| {
                world
                    .materials()
                    .get(block.face(face).material_id() as usize)
                    .is_some_and(|material| {
                        material.flags & assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING != 0
                    })
            }) {
                continue;
            }
            let (mesh, optional) = if let Some((mesh, optional)) = meshes.get(&block_visual.0) {
                (Arc::clone(mesh), *optional)
            } else {
                // World model thumbnails retain full-resolution tiles and directional UVs.
                // Only authored carried or GUI sheets define the canonical cube shortcut.
                if icons
                    .block_sheets()
                    .binary_search_by_key(&block_visual.0, |sheet| sheet.visual.0)
                    .is_err()
                    && icons
                        .gui_block_sheets()
                        .binary_search_by_key(&block_visual.0, |sheet| sheet.visual.0)
                        .is_err()
                {
                    continue;
                }
                let Some(sprite) = sources.get(&block_visual.0) else {
                    continue;
                };
                if !cube_shape_admitted(&ordinary, block_visual.0) {
                    continue;
                }
                let opaque = ordinary_cube_sheet(&sprite.rgba8);
                // Normalize alpha-tested faces using their material mode. Fractional blended
                // faces retain the thumbnail's composited texels.
                let Some(sprite) = gui_cube_sheet(world, block_visual, sprite) else {
                    continue;
                };
                // Optional sources follow required textures so discarding their meshes can
                // reclaim their pages. A full atlas leaves the original thumbnail intact.
                let Some(mesh) = atlas.try_insert(
                    [([sprite.width, sprite.height], sprite.rgba8.as_ref())],
                    |icons| {
                        let build = if opaque {
                            item_gui::cube
                        } else {
                            item_gui::cutout_cube
                        };
                        build(sheet_faces(icons[0]), lighting_for_visual(block_visual.0))
                    },
                ) else {
                    continue;
                };
                meshes.insert(block_visual.0, (Arc::clone(&mesh), true));
                (mesh, true)
            };
            models.insert(key, mesh);
            if optional {
                optional_models.insert(key);
                if sources
                    .get(&block_visual.0)
                    .is_some_and(|sprite| !ordinary_cube_sheet(&sprite.rgba8))
                {
                    faded_thumbnails.insert(key);
                }
            }
        }
        if let Some(refs) = self.icon_refs.as_deref() {
            let render_kinds: BTreeMap<_, _> = icons
                .entries()
                .iter()
                .map(|entry| {
                    (
                        entry.sprite,
                        assets::facts::block_render_fact(&entry.identifier).kind,
                    )
                })
                .collect();
            for thumbnail in icons.block_models() {
                // Independent GUI artwork must not fall back to the held world model.
                if gui_thumbnails.contains(&thumbnail.sprite) {
                    continue;
                }
                if let Some(icon) = refs.get(thumbnail.sprite as usize)
                    && !models.contains_key(&icon_key(*icon))
                    && let Some(mesh) = block_models::mesh(
                        world,
                        thumbnail.visual,
                        &mut atlas,
                        render_kinds
                            .get(&thumbnail.sprite)
                            .copied()
                            .unwrap_or_default(),
                    )
                {
                    models.insert(icon_key(*icon), mesh);
                    optional_models.insert(icon_key(*icon));
                    faded_thumbnails.insert(icon_key(*icon));
                }
            }
        }
        let (pages, mut textures) = atlas.finish()?;
        if let Some(refs) = self.icon_refs.as_deref() {
            for (sprite, icon) in icons.sprites().iter().zip(refs) {
                textures
                    .entry(atlas::key([sprite.width, sprite.height], &sprite.rgba8))
                    .or_insert(*icon);
            }
        }
        tracing::info!(
            models = models.len(),
            source_pages = pages.len(),
            "native GUI item geometry prepared"
        );
        for hat in cosmetics::Item::all() {
            render_model::hats::rig(hat);
        }
        Ok(PreparedGuiModels {
            shield,
            color_masks: super::super::color_icons::prepare(
                entities,
                icons,
                self.icon_refs.as_deref().unwrap_or_default(),
            ),
            pages,
            models,
            optional_models,
            faded_thumbnails,
            required_pages,
            textures,
            held,
            held_texture_fallbacks,
            hand_pivots: held::player_hands(entities),
            cape: render_model::cape::CapeRig::resolve(entities),
        })
    }
}
