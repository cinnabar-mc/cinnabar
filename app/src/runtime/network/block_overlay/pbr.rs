//! Server texture sets' normal, height and material layers for the Enhanced renderer.
//!
//! A static overlay texture whose colour came from a pack texture set with surface layers is
//! decoded again into authored PBR layers once the overlay's colour pages are placed. The layers
//! go into sparse Enhanced pages keyed by the overlay texture reference, charged to the same
//! terrain byte budget as the colour pages. Builds that cannot draw Enhanced skip the work, and
//! a set that no longer fits keeps only its colour.

use std::collections::BTreeMap;

use assets::{TextureRef, texture::EnhancedTextureAssets};
use pack_compiler::pbr::{PbrPages, build_pbr_mips, pbr_texture_from_texture_set};
use resource_pack::LayeredPackView;

use super::Builder;

/// Whether this build decodes server texture-set surfaces; only the Enhanced renderer reads
/// them. Tests always do, so the CPU path stays covered while Enhanced is disabled.
pub(super) const DECODE_SERVER_SURFACES: bool =
    render_model::ENHANCED_RENDERING_ENABLED || cfg!(test);

#[cfg(test)]
thread_local! {
    /// Caps the surface budget for the current test thread, to exercise exhaustion.
    pub(super) static SURFACE_BUDGET_LIMIT: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// Static overlay textures whose colour came from a pack texture set, by texture reference.
#[derive(Default)]
pub(super) struct PbrCandidates(BTreeMap<u32, (String, Option<[u8; 3]>)>);

impl PbrCandidates {
    /// Records the overlay texture `texture` as the colour of the texture set at `path`.
    pub(super) fn record(&mut self, texture: TextureRef, path: &str, tint: Option<[u8; 3]>) {
        if DECODE_SERVER_SURFACES {
            self.0
                .entry(texture.raw())
                .or_insert_with(|| (path.to_owned(), tint));
        }
    }
}

impl Builder<'_> {
    /// Decodes the recorded texture sets' surfaces into the overlay's sparse Enhanced pages,
    /// in texture-reference order, until the terrain byte budget is spent.
    pub(super) fn build_server_surfaces(&mut self, view: &LayeredPackView) {
        let candidates = std::mem::take(&mut self.pbr.0);
        let mut pages = PbrPages::default();
        let mut references = BTreeMap::new();
        let budget = self.surface_budget();
        let mut remaining = budget;
        let flags = self.texture_flags();
        for (texture, (path, tint)) in candidates {
            let Some(set) = pack_runtime::pack_textures::PackTextureSet::find(view, &path) else {
                continue;
            };
            if !pack_compiler::pbr::texture_set_has_surface(&set.definition) {
                continue;
            }
            let decoded =
                pbr_texture_from_texture_set(&set.definition, &mut |name| set.image(name));
            let page = match &decoded {
                Ok(texture) => PbrPages::page_for(texture.native_frame_size()),
                Err(_) => 0,
            };
            let (cutout, overlay_mask) = flags.get(&texture).copied().unwrap_or_default();
            let layer = decoded.and_then(|decoded| {
                let frame = decoded.frame_with_inherited_tint(0, 1, tint)?;
                build_pbr_mips(&frame, pages.tile_size(page), cutout, overlay_mask)
            });
            let cost = pages.layer_bytes(page);
            match layer {
                Ok(layer) if pages.has_room(page) && cost <= remaining => {
                    remaining -= cost;
                    references.insert(texture, pages.push(page, &layer));
                }
                _ => self.gaps.downgraded_texture_sets += 1,
            }
        }
        // An unreferenced page needs only a placeholder layer.
        let Some([color, normal, material]) = pages.finish(|_| 1) else {
            return;
        };
        self.upload_bytes += budget - remaining;
        self.overlay.enhanced = EnhancedTextureAssets::new(
            color,
            normal,
            material,
            EnhancedTextureAssets::carrier_references(references),
        )
        .map(std::sync::Arc::new);
    }

    /// Terrain bytes the surface layers may still use: what the colour pages left.
    fn surface_budget(&self) -> usize {
        let budget = self
            .texture_policy
            .storage_bytes()
            .saturating_sub(self.upload_bytes);
        #[cfg(test)]
        if let Some(limit) = SURFACE_BUDGET_LIMIT.get() {
            return budget.min(limit);
        }
        budget
    }

    /// Whether any overlay material drawing each raw texture reference cuts out or masks a
    /// tint overlay.
    fn texture_flags(&self) -> std::collections::HashMap<u32, (bool, bool)> {
        let mut flags = std::collections::HashMap::<u32, (bool, bool)>::new();
        for material in &self.overlay.materials {
            let (cutout, mask) = flags.entry(material.texture.raw()).or_default();
            *cutout |= material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT != 0;
            *mask |= material.flags & assets::MATERIAL_FLAG_OVERLAY_MASK != 0;
        }
        flags
    }
}
