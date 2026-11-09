use std::sync::Arc;

use super::SkinRaster;
use bedrock_protocol_1_26_60::enums::{PersonaAnimatedTextureType, PersonaAnimationExpression};
#[cfg(test)]
use bedrock_protocol_1_26_60::types::AnimatedImageData;

use crate::MAX_PLAYER_LIST_SKIN_BYTES;

use render_api::{SkinAnimation, SkinAnimationKind};

/// Borrowed animation facts independent of the packet's generated skin type.
pub(super) struct AnimationView<'a> {
    pub raster: SkinRaster<'a>,
    pub kind: u32,
    pub frames: f32,
    pub expression: u32,
}

/// Retains valid transmitted atlases within the same budget as the base skin.
pub(super) fn normalize<'a>(
    images: impl IntoIterator<Item = AnimationView<'a>>,
    retained: &mut usize,
) -> Arc<[SkinAnimation]> {
    let mut output: Vec<SkinAnimation> = Vec::new();
    for image in images {
        let kind = match PersonaAnimatedTextureType::from(image.kind) {
            PersonaAnimatedTextureType::Face => SkinAnimationKind::Face,
            PersonaAnimatedTextureType::Body32x32 => SkinAnimationKind::Body32,
            PersonaAnimatedTextureType::Body128x128 => SkinAnimationKind::Body128,
            _ => continue,
        };
        let raster = &image.raster;
        let (width, height, frames) = (raster.width, raster.height, image.frames);
        let Some(bytes) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            continue;
        };
        let previous = output.iter().position(|image| image.kind == kind);
        let released = previous.map_or(0, |index| output[index].rgba8.len());
        let candidate_bytes = retained
            .checked_sub(released)
            .and_then(|retained| retained.checked_add(bytes));
        if width == 0
            || height == 0
            || bytes != raster.bytes.len()
            || !frames.is_finite()
            || frames < 1.0
            || frames > height as f32
            || frames.fract() != 0.0
            || candidate_bytes.is_none_or(|total| total > MAX_PLAYER_LIST_SKIN_BYTES)
        {
            continue;
        }
        if let Some(previous) = previous {
            output.remove(previous);
        }
        *retained = candidate_bytes.expect("validated animation byte total");
        output.push(SkinAnimation {
            kind,
            width,
            height,
            rgba8: raster.bytes.as_ref().into(),
            frames: frames as u32,
            blinking: image.expression == PersonaAnimationExpression::Blinking.to_raw(),
        });
    }
    output.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock_protocol_1_26_60::types::SkinImage;
    use bedrock_protocol_1_26_60::wire;

    /// Adapts player-list fixture atlases to the shared borrowed animation input.
    fn normalize(images: &[AnimatedImageData], retained: &mut usize) -> Arc<[SkinAnimation]> {
        super::normalize(
            images.iter().map(|image| AnimationView {
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

    #[test]
    fn review_animation_replacement_refunds_its_previous_atlas_before_budgeting() {
        let image = |height, byte| AnimatedImageData {
            skin_image: SkinImage {
                width: wire::U32LE(1),
                height: wire::U32LE(height),
                image_bytes: vec![byte; height as usize * 4].into(),
            },
            animated_texture_type: PersonaAnimatedTextureType::Body32x32,
            frames: wire::F32LE(1.0),
            animation_expression: PersonaAnimationExpression::Linear,
        };
        let mut retained = MAX_PLAYER_LIST_SKIN_BYTES - 8;
        let images = normalize(&[image(2, 1), image(1, 2)], &mut retained);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].height, 1);
        assert_eq!(images[0].rgba8.as_ref(), &[2; 4]);
        assert_eq!(retained, MAX_PLAYER_LIST_SKIN_BYTES - 4);
    }

    #[test]
    fn transmitted_persona_animation_keeps_its_actual_width_and_frame_count() {
        let input = AnimatedImageData {
            skin_image: SkinImage {
                width: wire::U32LE(24),
                height: wire::U32LE(512),
                image_bytes: vec![71; 24 * 512 * 4].into(),
            },
            animated_texture_type: PersonaAnimatedTextureType::Body32x32,
            frames: wire::F32LE(16.0),
            animation_expression: PersonaAnimationExpression::Linear,
        };
        let mut bytes = 0;
        let images = normalize(&[input], &mut bytes);
        assert_eq!(images.len(), 1);
        assert_eq!((images[0].width, images[0].frames), (24, 16));
        assert_eq!(bytes, images[0].rgba8.len());
        assert_eq!(images[0].kind.geometry_key(), "animated_32x32");
    }
}
