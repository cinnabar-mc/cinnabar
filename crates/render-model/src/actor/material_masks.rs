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

    /// Places `masks` beside a draw at `base`, preferring a pack mask's copy on the base's page:
    /// a mask on another page than its base texture cannot be sampled by the same draw and is
    /// left out.
    #[must_use]
    pub fn resolve(
        artwork: &ActorArtworkPages,
        rig: EntityRigId,
        base: ActorArtworkLocation,
        masks: &assets::EntityMaterialMasks,
    ) -> Self {
        let layer = |source: Option<u32>| {
            source
                .and_then(|source| {
                    let copy = crate::is_pack_rig_id(rig)
                        .then(|| artwork.pack_mask_layer(base.page, source))
                        .flatten();
                    copy.or_else(|| {
                        artwork
                            .variant_location(rig, source)
                            .filter(|location| location.page == base.page)
                            .map(|location| location.layer)
                    })
                })
                .unwrap_or(u32::MAX)
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

    #[test]
    fn a_page_split_keeps_masks_beside_their_base() {
        let texture = |source: u32| assets::ActorTexture {
            source,
            width: 1,
            height: 1,
            pixel_sha256: [0; 32],
            rgba8: vec![source as u8; 4].into(),
        };
        // One page short of room for both: the base fills the first page's last layer.
        let mut textures: Vec<_> = (0..255).map(texture).collect();
        let base_index = textures.len() as u32;
        textures.extend([texture(1000), texture(1001), texture(1002)]);
        let binding = assets::ActorArtworkBinding {
            rig: 0,
            geometry_candidate: 0,
            entity_symbol: 0,
            geometry: 0,
            render_controller: 0,
            texture: base_index,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        };
        let sharing = assets::ActorPageSharing {
            companions: [(1000, 1000), (1001, 1000), (1002, 1000)].into(),
            ..Default::default()
        };
        let artwork =
            ActorArtworkPages::default().with_pack_artwork(&textures, &[binding], &sharing);
        assert!(
            artwork.pages().len() > 1,
            "the fixture must split its group"
        );
        let rig = crate::pack_rig_id(0);
        let base = artwork.variant_location(rig, 1000).unwrap();
        let masks = ActorMaterialMasks::resolve(
            &artwork,
            rig,
            base,
            &assets::EntityMaterialMasks {
                glow: Some(1001),
                emissive: Some(1002),
                strength: 1.0,
            },
        );
        assert_eq!(
            masks.glow_layer,
            artwork.variant_location(rig, 1001).unwrap().layer
        );
        assert_eq!(
            masks.emissive_layer,
            artwork.variant_location(rig, 1002).unwrap().layer
        );
        for source in 0..255 {
            assert!(artwork.variant_location(rig, source).is_some());
        }
    }

    #[test]
    fn a_mask_shared_past_one_page_is_repeated_beside_every_base() {
        let texture = |source: u32| assets::ActorTexture {
            source,
            width: 1,
            height: 1,
            pixel_sha256: [0; 32],
            rgba8: vec![source as u8; 4].into(),
        };
        // More same-size bases than one page holds, all drawn with one glow mask.
        let bases: Vec<u32> = (0..300).collect();
        let mut textures: Vec<_> = bases.iter().copied().map(texture).collect();
        textures.push(texture(1000));
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
        let sharing = assets::ActorPageSharing {
            companions: bases
                .iter()
                .chain(&[1000])
                .map(|&source| (source, 0))
                .collect(),
            masks: [1000].into(),
            ..Default::default()
        };
        let artwork =
            ActorArtworkPages::default().with_pack_artwork(&textures, &[binding], &sharing);
        assert!(artwork.pages().len() > 1, "the fixture must span pages");
        let rig = crate::pack_rig_id(0);
        let glow = assets::EntityMaterialMasks {
            glow: Some(1000),
            emissive: None,
            strength: 1.0,
        };
        for &source in &bases {
            let base = artwork.variant_location(rig, source).unwrap();
            let masks = ActorMaterialMasks::resolve(&artwork, rig, base, &glow);
            let page = &artwork.pages()[usize::from(base.page) - 1];
            assert!(
                masks.glow_layer < page.layers,
                "base {source} keeps its mask"
            );
            let texel = masks.glow_layer as usize * 4;
            assert_eq!(page.pixels()[texel], 1000u32 as u8, "the layer is the mask");
        }
    }
}
