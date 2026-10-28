use std::sync::Arc;

use bedrock_protocol_1_26_60::{
    types::{SerializedSkin, SerializedSkinRef},
    wire,
};

use super::MAX_PLAYER_LIST_SKIN_BYTES;
use render_api::{
    CAPE_DIMENSIONS, CLASSIC_SKIN_SIDE, CapeImage, MAX_CLASSIC_SKIN_SIDE,
    MAX_SKIN_GEOMETRY_SOURCE_BYTES, MAX_STANDARD_SKIN_SIDE, SkinGeometrySource, SkinRgba8,
    StandardSkin, expand_legacy_skin_rgba8,
};

mod alpha;
pub use alpha::{normalize_classic_skin_rgba8, normalize_custom_skin_rgba8};
mod animation;

/// Borrowed image facts shared by the two generated skin layouts.
pub(super) struct SkinRaster<'a> {
    pub width: u32,
    pub height: u32,
    pub bytes: &'a [u8],
}

/// Supplies rendering inputs from a player-list or appearance-update skin.
pub(super) trait SkinSource {
    /// Returns the base skin raster.
    fn image(&self) -> SkinRaster<'_>;
    /// Returns the optional cape raster.
    fn cape(&self) -> SkinRaster<'_>;
    /// Returns the resource patch and model data without copying them.
    fn geometry(&self) -> (&wire::Str, &wire::Str);
    /// Reports whether the raster belongs to an assembled persona.
    fn persona(&self) -> bool;
    /// Retains transmitted animation atlases within the shared skin budget.
    fn animations(&self, retained: &mut usize) -> Arc<[render_api::SkinAnimation]>;
}

macro_rules! skin_source {
    ($kind:ty) => {
        impl SkinSource for $kind {
            fn image(&self) -> SkinRaster<'_> {
                SkinRaster {
                    width: self.image_data.width.0,
                    height: self.image_data.height.0,
                    bytes: &self.image_data.image_bytes,
                }
            }
            fn cape(&self) -> SkinRaster<'_> {
                SkinRaster {
                    width: self.cape_image_data.width.0,
                    height: self.cape_image_data.height.0,
                    bytes: &self.cape_image_data.image_bytes,
                }
            }
            fn geometry(&self) -> (&wire::Str, &wire::Str) {
                (&self.resource_patch, &self.geometry_data)
            }
            fn persona(&self) -> bool {
                self.is_persona
            }
            fn animations(&self, retained: &mut usize) -> Arc<[render_api::SkinAnimation]> {
                animation::normalize(
                    self.animated_image_data
                        .iter()
                        .map(|image| animation::AnimationView {
                            raster: SkinRaster {
                                width: image.skin_image.width.0,
                                height: image.skin_image.height.0,
                                bytes: &image.skin_image.image_bytes,
                            },
                            kind: image.animated_texture_type.to_raw(),
                            frames: image.frames.0,
                            expression: image.animation_expression.to_raw(),
                        }),
                    retained,
                )
            }
        }
    };
}
skin_source!(SerializedSkinRef);
skin_source!(SerializedSkin);

/// Keeps the resource patch even without model data, so classic slim skins select their model.
fn geometry_source(
    skin: &impl SkinSource,
    retained_bytes: &mut usize,
) -> Option<Arc<SkinGeometrySource>> {
    let (resource_patch, geometry_data) = skin.geometry();
    let bytes = resource_patch.len() + geometry_data.len();
    let next = retained_bytes.checked_add(bytes)?;
    if resource_patch.is_empty()
        || bytes > MAX_SKIN_GEOMETRY_SOURCE_BYTES
        || next > MAX_PLAYER_LIST_SKIN_BYTES
    {
        return None;
    }
    *retained_bytes = next;
    Some(Arc::new(SkinGeometrySource {
        resource_patch: resource_patch.to_string_lossy().into(),
        geometry_data: geometry_data.to_string_lossy().into(),
        animations: skin.animations(retained_bytes),
    }))
}

/// Retains a supported cape raster within the shared skin budget.
fn normalize_cape(image: SkinRaster<'_>, retained_bytes: &mut usize) -> Option<CapeImage> {
    let (width, height) = (image.width, image.height);
    if !CAPE_DIMENSIONS.contains(&(width, height)) {
        return None;
    }
    let expected = usize::try_from(width).ok()? * usize::try_from(height).ok()? * 4;
    let next = retained_bytes.checked_add(expected)?;
    if image.bytes.len() != expected || next > MAX_PLAYER_LIST_SKIN_BYTES {
        return None;
    }
    *retained_bytes = next;
    Some(CapeImage {
        width,
        height,
        rgba8: Arc::from(image.bytes),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSkinUnavailable {
    InvalidDimensions,
    InvalidByteLength,
    RetainedBudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerSkin {
    Standard(StandardSkin),
    Unavailable(PlayerSkinUnavailable),
}

pub(super) fn normalize_player_skin(
    skin: impl SkinSource,
    retained_bytes: &mut usize,
) -> PlayerSkin {
    // Serialized persona payloads carry the assembled model, base raster and animation atlases.
    let image = skin.image();
    let (width, mut height) = (image.width, image.height);
    let classic = CLASSIC_SKIN_SIDE as u32;
    let legacy = (width, height) == (classic, classic / 2);
    let limit = if skin.persona() {
        MAX_STANDARD_SKIN_SIDE
    } else {
        MAX_CLASSIC_SKIN_SIDE as u32
    };
    if !legacy && (width != height || !width.is_power_of_two() || width < classic || width > limit)
    {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    }
    let Some(expected_bytes) = usize::try_from(width)
        .ok()
        .and_then(|width| usize::try_from(height).ok().map(|height| (width, height)))
        .and_then(|(width, height)| width.checked_mul(height))
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    };
    if image.bytes.len() != expected_bytes {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidByteLength);
    }
    let mut rgba8 = if legacy && !skin.persona() {
        height = width;
        expand_legacy_skin_rgba8(image.bytes, width as usize)
    } else {
        image.bytes.to_vec()
    };
    let expected_bytes = rgba8.len();
    let Some(next_bytes) = retained_bytes.checked_add(expected_bytes) else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded);
    };
    if next_bytes > MAX_PLAYER_LIST_SKIN_BYTES {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded);
    }
    *retained_bytes = next_bytes;
    let cape = normalize_cape(skin.cape(), retained_bytes);
    let geometry = geometry_source(&skin, retained_bytes);
    alpha::normalize(skin.persona(), skin.geometry().0, width, height, &mut rgba8);
    PlayerSkin::Standard(StandardSkin {
        width,
        height,
        rgba8: SkinRgba8::from(rgba8),
        cape,
        geometry,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock_protocol_1_26_60::{types::SkinImage, wire};

    #[test]
    fn null_geometry_keeps_the_captured_slim_resource_patch() {
        let skin = SerializedSkinRef {
            resource_patch: r#"{"geometry":{"default":"geometry.humanoid.customSlim"}}"#.into(),
            geometry_data: "null".into(),
            ..Default::default()
        };
        let mut bytes = 0;
        let source = geometry_source(&skin, &mut bytes).expect("named classic model survives");
        assert_eq!(skin.resource_patch, source.resource_patch.as_ref());
        assert_eq!(source.geometry_data.as_ref(), "null");
        assert_eq!(bytes, source.byte_len());
    }

    #[test]
    fn classic_rejects_large_rasters_while_persona_accepts_the_bounded_larger_atlas() {
        for side in [MAX_CLASSIC_SKIN_SIDE as u32 * 2, MAX_STANDARD_SKIN_SIDE] {
            let mut source = SerializedSkinRef {
                image_data: SkinImage {
                    width: wire::U32LE(side),
                    height: wire::U32LE(side),
                    image_bytes: vec![255; (side * side * 4) as usize].into(),
                },
                ..Default::default()
            };
            assert_eq!(
                normalize_player_skin(source.clone(), &mut 0),
                PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions)
            );
            source.is_persona = true;
            assert!(matches!(
                normalize_player_skin(source, &mut 0),
                PlayerSkin::Standard(_)
            ));
        }
    }

    #[test]
    fn legacy_alpha_is_validated_after_left_limbs_are_expanded() {
        let side = CLASSIC_SKIN_SIDE as u32;
        let source = SerializedSkinRef {
            resource_patch: r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#.into(),
            image_data: SkinImage {
                width: wire::U32LE(side),
                height: wire::U32LE(side / 2),
                image_bytes: vec![0; (side * side / 2 * 4) as usize].into(),
            },
            ..Default::default()
        };
        let PlayerSkin::Standard(skin) = normalize_player_skin(source, &mut 0) else {
            panic!("legacy skin");
        };
        assert_eq!(skin.height, side);
        assert_eq!(skin.rgba8[(52 * CLASSIC_SKIN_SIDE + 16) * 4 + 3], 255);
        assert_eq!(skin.rgba8[(48 * CLASSIC_SKIN_SIDE) * 4 + 3], 0);
    }
}
