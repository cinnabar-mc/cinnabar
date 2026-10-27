//! Cinnabar material masks on a draw's own artwork page.

use crate::{ActorArtworkLocation, ActorArtworkPages, EntityRigId};

/// Texture layers of a Cinnabar material's masks on its base artwork page, and the strength that
/// scales them. `u32::MAX` names no mask.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorMaterialMasks {
    /// Added after lighting, weighted by its alpha and `strength`.
    pub glow_layer: u32,
    /// Draws masked texels unlit, weighted by its alpha and `strength`.
    pub emissive_layer: u32,
    pub strength: f32,
}

impl ActorMaterialMasks {
    pub const NONE: Self = Self {
        glow_layer: u32::MAX,
        emissive_layer: u32::MAX,
        strength: 0.0,
    };

    /// Places `masks` beside a draw at `base`: a mask on another page than its base texture
    /// cannot be sampled by the same draw and is left out.
    #[must_use]
    pub fn resolve(
        artwork: &ActorArtworkPages,
        rig: EntityRigId,
        base: ActorArtworkLocation,
        masks: &assets::EntityMaterialMasks,
    ) -> Self {
        let layer = |source: Option<u32>| {
            source
                .and_then(|source| artwork.variant_location(rig, source))
                .filter(|location| location.page == base.page)
                .map_or(u32::MAX, |location| location.layer)
        };
        Self {
            glow_layer: layer(masks.glow),
            emissive_layer: layer(masks.emissive),
            strength: masks.strength,
        }
    }
}

impl Default for ActorMaterialMasks {
    fn default() -> Self {
        Self::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_resolve_only_on_their_base_page() {
        let texture = |source: u32, side: u16| assets::ActorTexture {
            source,
            width: side,
            height: side,
            pixel_sha256: [source as u8; 32],
            rgba8: vec![source as u8; usize::from(side) * usize::from(side) * 4].into(),
        };
        let binding = assets::ActorArtworkBinding {
            rig: 0,
            geometry_candidate: 0,
            entity_symbol: 0,
            geometry: 0,
            render_controller: 0,
            texture: 0,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        };
        // Equal sizes share a page; the 4x4 mask lands on another one.
        let textures = [texture(1, 2), texture(2, 2), texture(3, 4)];
        let artwork = ActorArtworkPages::default().with_pack_artwork(
            &textures,
            &[binding],
            &Default::default(),
        );
        let rig = crate::pack_rig_id(0);
        let base = artwork.variant_location(rig, 1).unwrap();
        let masks = ActorMaterialMasks::resolve(
            &artwork,
            rig,
            base,
            &assets::EntityMaterialMasks {
                glow: Some(2),
                emissive: Some(3),
                strength: 1.5,
            },
        );
        assert_eq!(
            masks.glow_layer,
            artwork.variant_location(rig, 2).unwrap().layer
        );
        assert_eq!(masks.emissive_layer, u32::MAX);
        assert_eq!(masks.strength, 1.5);
    }
}
