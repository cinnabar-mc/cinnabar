//! Selected player-controller geometry and artwork for level-view paper dolls.

use super::{
    super::UiPresentationRuntime,
    live_player::{ModelGeometry, render_bone},
};
use glam::{Affine3A, Vec3};
use render_model::{ActorRigGeometry, EntityRigId, UiTexturePage};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use ui::IconRef;

#[derive(Default)]
pub(in super::super) struct PlayerLayers {
    source: Option<Arc<assets::SessionEntityPack>>,
    geometries: BTreeMap<u32, ActorRigGeometry>,
    selected: Vec<u32>,
    pub(in super::super) pages: Vec<UiTexturePage>,
    icons: BTreeMap<u32, IconRef>,
    pub(super) layers: Vec<(ModelGeometry, actor_animation::RenderTextureLayer, IconRef)>,
}

impl PlayerLayers {
    pub(super) fn has_layers(&self) -> bool {
        !self.layers.is_empty()
    }
}

impl UiPresentationRuntime {
    /// Replaces the immutable catalog; geometry and texture identities never cross pack changes.
    pub fn set_preview_player_layers(&mut self, source: Option<Arc<assets::SessionEntityPack>>) {
        let old = &self.gui_models.player_layers.source;
        if match (old, &source) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        } {
            return;
        }
        self.gui_models.player_layers = PlayerLayers {
            source,
            ..Default::default()
        };
        self.rebuild_dynamic_textures();
    }

    /// Whether this session can supply authored player-controller layers to HUD controls.
    pub fn has_preview_player_layers(&self) -> bool {
        self.gui_models.player_layers.source.is_some()
    }

    pub(super) fn capture_player_layers(
        &mut self,
        local: Option<client_world::ui_read::LocalPlayerUi<'_>>,
    ) {
        let layers = &mut self.gui_models.player_layers;
        layers.layers.clear();
        let Some(source) = layers.source.as_ref() else {
            return;
        };
        let Some(local) = local else { return };
        let Some(rig) = local.rig() else { return };
        let Some(output) = local.paper_doll_layers() else {
            return;
        };
        // Source indices belong to this exact entity catalog, including after a live pack reload.
        let Some((catalog, _)) = rig.geometry_source() else {
            return;
        };
        if catalog.source_manifest_sha256() != source.assets.source_manifest_sha256() {
            return;
        }
        let wanted: Vec<_> = output
            .render
            .iter()
            .filter(|layer| layer.geometry.is_some())
            .map(|layer| layer.source)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let changed = wanted != layers.selected;
        if changed {
            layers.selected = wanted;
            layers.pages.clear();
            layers.icons.clear();
            let available = (render_model::MAX_UI_TEXTURE_LAYERS as usize).saturating_sub(
                self.textures.dynamic_start() + render_model::UI_SELECTED_MODEL_PAGE_OFFSET,
            );
            let selected = layers
                .selected
                .iter()
                .filter_map(|id| source.textures.iter().find(|texture| texture.source == *id))
                .collect::<Vec<_>>();
            let bytes: usize = selected.iter().map(|texture| texture.rgba8.len()).sum();
            if selected.len() <= available && bytes <= render_model::MAX_UI_NATIVE_SHEET_BYTES {
                for texture in selected {
                    let size = [u32::from(texture.width), u32::from(texture.height)];
                    let Ok(page) = UiTexturePage::owned(size, Arc::clone(&texture.rgba8)) else {
                        continue;
                    };
                    layers.icons.insert(
                        texture.source,
                        IconRef {
                            page: (self.textures.dynamic_start()
                                + render_model::UI_SELECTED_MODEL_PAGE_OFFSET
                                + layers.pages.len()) as u16,
                            uv: [0, 0, texture.width, texture.height],
                            color_mask: None,
                            glint: false,
                        },
                    );
                    layers.pages.push(page);
                }
            } else {
                tracing::warn!(
                    textures = selected.len(),
                    bytes,
                    "selected player artwork exceeds the UI catalog budget"
                );
            }
        }
        let frame = Affine3A::from_scale(
            Vec3::new(-1., 1., -1.)
                * Vec3::from_array(rig.axis_scale)
                * (rig.scale / super::super::player_preview::PLAYER_MODEL_SCALE),
        );
        for layer in output.render.iter() {
            let Some(id) = layer.geometry else { continue };
            let Some(icon) = layers.icons.get(&layer.source).copied() else {
                continue;
            };
            if let std::collections::btree_map::Entry::Vacant(entry) = layers.geometries.entry(id) {
                let Ok(geometry) = render_model::entity_geometry(
                    &source.assets,
                    id as usize,
                    EntityRigId(rig.rig.0),
                ) else {
                    continue;
                };
                entry.insert(geometry);
            }
            let Some(pose) = layer
                .pose
                .iter()
                .map(render_bone)
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let mut model = ModelGeometry::default();
            model.capture(&layers.geometries[&id], &pose, frame);
            for bone in layer.hidden_bones.iter().copied() {
                if let Some(transform) = model.transforms.get_mut(bone as usize) {
                    *transform = Affine3A::from_scale(Vec3::ZERO);
                }
            }
            if model.geometry.is_some() {
                layers.layers.push((model, layer.clone(), icon));
            }
        }
        if changed {
            self.rebuild_dynamic_textures();
            let layers = &mut self.gui_models.player_layers;
            let first = self.textures.dynamic_start() + render_model::UI_SELECTED_MODEL_PAGE_OFFSET;
            let admitted = layers.pages.iter().enumerate().all(|(index, page)| {
                self.textures
                    .pages()
                    .get(first + index)
                    .is_some_and(|placed| placed == page)
            });
            if !admitted {
                layers.layers.clear();
                layers.icons.clear();
                layers.selected.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests;
