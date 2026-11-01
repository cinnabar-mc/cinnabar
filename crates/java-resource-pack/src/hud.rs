//! HUD textures: Java's `gui/icons.png` and `gui/widgets.png` atlases, the 1.20.2+ HUD sprites
//! that replaced them, and the individual textures the Bedrock HUD draws.

use crate::texture::{canvas, crop, is_empty, paste};
use image::RgbaImage;
use std::collections::BTreeMap;

/// Java 1.20.2+ HUD sprites (paths below `gui/sprites/hud`) and the Bedrock textures they become.
const SPRITE_UI: [(&str, &str); 50] = [
    ("heart/container", "textures/ui/heart_background.png"),
    ("heart/container_blinking", "textures/ui/heart_blink.png"),
    ("heart/full", "textures/ui/heart.png"),
    ("heart/half", "textures/ui/heart_half.png"),
    ("heart/full_blinking", "textures/ui/heart_flash.png"),
    ("heart/half_blinking", "textures/ui/heart_flash_half.png"),
    ("heart/poisoned_full", "textures/ui/poison_heart.png"),
    ("heart/poisoned_half", "textures/ui/poison_heart_half.png"),
    (
        "heart/poisoned_full_blinking",
        "textures/ui/poison_heart_flash.png",
    ),
    (
        "heart/poisoned_half_blinking",
        "textures/ui/poison_heart_flash_half.png",
    ),
    ("heart/withered_full", "textures/ui/wither_heart.png"),
    ("heart/withered_half", "textures/ui/wither_heart_half.png"),
    (
        "heart/withered_full_blinking",
        "textures/ui/wither_heart_flash.png",
    ),
    (
        "heart/withered_half_blinking",
        "textures/ui/wither_heart_flash_half.png",
    ),
    ("heart/absorbing_full", "textures/ui/absorption_heart.png"),
    (
        "heart/absorbing_half",
        "textures/ui/absorption_heart_half.png",
    ),
    ("heart/frozen_full", "textures/ui/freeze_heart.png"),
    ("heart/frozen_half", "textures/ui/freeze_heart_half.png"),
    (
        "heart/frozen_full_blinking",
        "textures/ui/freeze_heart_flash.png",
    ),
    (
        "heart/frozen_half_blinking",
        "textures/ui/freeze_heart_flash_half.png",
    ),
    ("heart/hardcore_full", "textures/ui/hardcore/heart.png"),
    ("heart/hardcore_half", "textures/ui/hardcore/heart_half.png"),
    (
        "heart/hardcore_full_blinking",
        "textures/ui/hardcore/heart_flash.png",
    ),
    (
        "heart/hardcore_half_blinking",
        "textures/ui/hardcore/heart_flash_half.png",
    ),
    (
        "heart/poisoned_hardcore_full",
        "textures/ui/hardcore/poison_heart.png",
    ),
    (
        "heart/poisoned_hardcore_half",
        "textures/ui/hardcore/poison_heart_half.png",
    ),
    (
        "heart/poisoned_hardcore_full_blinking",
        "textures/ui/hardcore/poison_heart_flash.png",
    ),
    (
        "heart/poisoned_hardcore_half_blinking",
        "textures/ui/hardcore/poison_heart_flash_half.png",
    ),
    (
        "heart/withered_hardcore_full",
        "textures/ui/hardcore/wither_heart.png",
    ),
    (
        "heart/withered_hardcore_half",
        "textures/ui/hardcore/wither_heart_half.png",
    ),
    (
        "heart/withered_hardcore_full_blinking",
        "textures/ui/hardcore/wither_heart_flash.png",
    ),
    (
        "heart/withered_hardcore_half_blinking",
        "textures/ui/hardcore/wither_heart_flash_half.png",
    ),
    (
        "heart/absorbing_hardcore_full",
        "textures/ui/hardcore/absorption_heart.png",
    ),
    (
        "heart/absorbing_hardcore_half",
        "textures/ui/hardcore/absorption_heart_half.png",
    ),
    (
        "heart/frozen_hardcore_full",
        "textures/ui/hardcore/freeze_heart.png",
    ),
    (
        "heart/frozen_hardcore_half",
        "textures/ui/hardcore/freeze_heart_half.png",
    ),
    (
        "heart/frozen_hardcore_full_blinking",
        "textures/ui/hardcore/freeze_heart_flash.png",
    ),
    (
        "heart/frozen_hardcore_half_blinking",
        "textures/ui/hardcore/freeze_heart_flash_half.png",
    ),
    ("food_empty", "textures/ui/hunger_background.png"),
    ("food_full", "textures/ui/hunger_full.png"),
    ("food_half", "textures/ui/hunger_half.png"),
    (
        "food_empty_hunger",
        "textures/ui/hunger_effect_background.png",
    ),
    ("food_full_hunger", "textures/ui/hunger_effect_full.png"),
    ("food_half_hunger", "textures/ui/hunger_effect_half.png"),
    ("armor_empty", "textures/ui/armor_empty.png"),
    ("armor_full", "textures/ui/armor_full.png"),
    ("armor_half", "textures/ui/armor_half.png"),
    ("air", "textures/ui/bubble.png"),
    ("air_bursting", "textures/ui/bubble_pop.png"),
    ("air_empty", "textures/ui/bubble_empty.png"),
];

/// A region of a 256x256 legacy GUI atlas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

/// Shorthand for an atlas region.
const fn rect(x: u32, y: u32, w: u32, h: u32) -> Rect {
    Rect { x, y, w, h }
}

/// Each 1.20.2+ HUD sprite's region in the `gui/icons.png` of packs made for 1.7-1.20.1; the
/// positions match the 1.20.1 atlas pixel for pixel.
pub(crate) const LEGACY_ICON_RECTS: [(&str, Rect); 61] = [
    ("crosshair", rect(0, 0, 15, 15)),
    ("heart/container", rect(16, 0, 9, 9)),
    ("heart/container_blinking", rect(25, 0, 9, 9)),
    ("heart/full", rect(52, 0, 9, 9)),
    ("heart/half", rect(61, 0, 9, 9)),
    ("heart/full_blinking", rect(70, 0, 9, 9)),
    ("heart/half_blinking", rect(79, 0, 9, 9)),
    ("heart/poisoned_full", rect(88, 0, 9, 9)),
    ("heart/poisoned_half", rect(97, 0, 9, 9)),
    ("heart/poisoned_full_blinking", rect(106, 0, 9, 9)),
    ("heart/poisoned_half_blinking", rect(115, 0, 9, 9)),
    ("heart/withered_full", rect(124, 0, 9, 9)),
    ("heart/withered_half", rect(133, 0, 9, 9)),
    ("heart/withered_full_blinking", rect(142, 0, 9, 9)),
    ("heart/withered_half_blinking", rect(151, 0, 9, 9)),
    ("heart/absorbing_full", rect(160, 0, 9, 9)),
    ("heart/absorbing_half", rect(169, 0, 9, 9)),
    ("heart/frozen_full", rect(178, 0, 9, 9)),
    ("heart/frozen_half", rect(187, 0, 9, 9)),
    ("armor_empty", rect(16, 9, 9, 9)),
    ("armor_half", rect(25, 9, 9, 9)),
    ("armor_full", rect(34, 9, 9, 9)),
    ("heart/vehicle_container", rect(52, 9, 9, 9)),
    ("heart/vehicle_full", rect(88, 9, 9, 9)),
    ("heart/vehicle_half", rect(97, 9, 9, 9)),
    ("air", rect(16, 18, 9, 9)),
    ("air_bursting", rect(25, 18, 9, 9)),
    ("food_empty", rect(16, 27, 9, 9)),
    ("food_full", rect(52, 27, 9, 9)),
    ("food_half", rect(61, 27, 9, 9)),
    ("food_full_hunger", rect(88, 27, 9, 9)),
    ("food_half_hunger", rect(97, 27, 9, 9)),
    ("food_empty_hunger", rect(133, 27, 9, 9)),
    ("heart/container_hardcore", rect(16, 45, 9, 9)),
    ("heart/container_hardcore_blinking", rect(25, 45, 9, 9)),
    ("heart/hardcore_full", rect(52, 45, 9, 9)),
    ("heart/hardcore_half", rect(61, 45, 9, 9)),
    ("heart/hardcore_full_blinking", rect(70, 45, 9, 9)),
    ("heart/hardcore_half_blinking", rect(79, 45, 9, 9)),
    ("heart/poisoned_hardcore_full", rect(88, 45, 9, 9)),
    ("heart/poisoned_hardcore_half", rect(97, 45, 9, 9)),
    ("heart/poisoned_hardcore_full_blinking", rect(106, 45, 9, 9)),
    ("heart/poisoned_hardcore_half_blinking", rect(115, 45, 9, 9)),
    ("heart/withered_hardcore_full", rect(124, 45, 9, 9)),
    ("heart/withered_hardcore_half", rect(133, 45, 9, 9)),
    ("heart/withered_hardcore_full_blinking", rect(142, 45, 9, 9)),
    ("heart/withered_hardcore_half_blinking", rect(151, 45, 9, 9)),
    ("heart/absorbing_hardcore_full", rect(160, 45, 9, 9)),
    ("heart/absorbing_hardcore_half", rect(169, 45, 9, 9)),
    ("heart/frozen_hardcore_full", rect(178, 45, 9, 9)),
    ("heart/frozen_hardcore_half", rect(187, 45, 9, 9)),
    ("experience_bar_background", rect(0, 64, 182, 5)),
    ("experience_bar_progress", rect(0, 69, 182, 5)),
    ("jump_bar_cooldown", rect(0, 74, 182, 5)),
    ("jump_bar_background", rect(0, 84, 182, 5)),
    ("jump_bar_progress", rect(0, 89, 182, 5)),
    ("hotbar_attack_indicator_background", rect(0, 94, 18, 18)),
    ("hotbar_attack_indicator_progress", rect(18, 94, 18, 18)),
    ("crosshair_attack_indicator_background", rect(36, 94, 16, 4)),
    ("crosshair_attack_indicator_progress", rect(52, 94, 16, 4)),
    ("crosshair_attack_indicator_full", rect(68, 94, 16, 16)),
];

/// The hotbar strip in a legacy `gui/widgets.png`.
pub(crate) const LEGACY_HOTBAR: Rect = rect(0, 0, 182, 22);
/// The hotbar selection in a legacy `gui/widgets.png`.
pub(crate) const LEGACY_SELECTION: Rect = rect(0, 22, 24, 24);

/// How many pixels of an HD atlas cover one pixel of its 256-pixel vanilla layout.
pub(crate) fn atlas_scale(atlas: &RgbaImage) -> u32 {
    (atlas.width() / 256).max(1)
}

/// Cuts `region`, scaled by `scale`, out of an atlas, within the derived-image budget.
pub(crate) fn crop_rect(atlas: &RgbaImage, region: Rect, scale: u32) -> Option<RgbaImage> {
    crop(
        atlas,
        region.x * scale,
        region.y * scale,
        region.w * scale,
        region.h * scale,
    )
}

/// Cuts the HUD sprites out of a legacy `gui/icons.png`, skipping regions the pack left empty
/// (frozen hearts, for example, which packs made before 1.17 do not have).
pub(crate) fn legacy_sprites(icons: &RgbaImage) -> BTreeMap<String, RgbaImage> {
    let scale = atlas_scale(icons);
    LEGACY_ICON_RECTS
        .iter()
        .filter_map(|(name, region)| {
            let sprite = crop_rect(icons, *region, scale)?;
            (!is_empty(&sprite)).then(|| ((*name).to_owned(), sprite))
        })
        .collect()
}

/// Converts HUD sprites, a hotbar strip and a hotbar selection into Bedrock UI textures.
pub(crate) fn hud_textures(
    sprites: &BTreeMap<String, RgbaImage>,
    hotbar: Option<&RgbaImage>,
    selection: Option<&RgbaImage>,
) -> Vec<(String, RgbaImage)> {
    let mut out: BTreeMap<String, RgbaImage> = BTreeMap::new();
    for (sprite, path) in SPRITE_UI {
        if let Some(image) = sprites.get(sprite) {
            out.insert(path.to_owned(), image.clone());
        }
    }
    if let Some(hotbar) = hotbar {
        // The 182-pixel Java hotbar is a 1-pixel cap, nine 20-pixel slots and another 1-pixel
        // cap; Bedrock stores each piece separately.
        let scale = (hotbar.width() / 182).max(1);
        let height = hotbar.height();
        let pieces = std::iter::once(("hotbar_start_cap".to_owned(), 0, scale))
            .chain((0..9).map(|slot| {
                (
                    format!("hotbar_{slot}"),
                    (1 + 20 * slot) * scale,
                    20 * scale,
                )
            }))
            .chain(std::iter::once((
                "hotbar_end_cap".to_owned(),
                181 * scale,
                scale,
            )));
        for (name, x, width) in pieces {
            if let Some(piece) = crop(hotbar, x, 0, width, height) {
                out.insert(format!("textures/ui/{name}.png"), piece);
            }
        }
    }
    if let Some(selection) = selection.and_then(square_selection) {
        out.insert("textures/ui/selected_hotbar_slot.png".into(), selection);
    }
    out.into_iter().collect()
}

/// Pads the 24x23 hotbar selection of 1.20.2+ to Bedrock's 24x24 by repeating its top border row
/// at the bottom, which is how the 24x24 legacy and Bedrock textures are drawn.
fn square_selection(selection: &RgbaImage) -> Option<RgbaImage> {
    let (width, height) = selection.dimensions();
    if width == height || width == 0 || height > width {
        return Some(selection.clone());
    }
    let scale = (width / 24).max(1);
    let mut square = canvas(width, width)?;
    paste(&mut square, selection, 0, 0);
    for y in height..width {
        let row = crop(selection, 0, (y - height) % scale, width, 1)?;
        paste(&mut square, &row, 0, y);
    }
    Some(square)
}

/// Rebuilds a legacy-layout `gui/icons.png` from 1.20.2+ sprites so the crosshair, which Bedrock
/// still reads from that atlas, is translated. The crosshair must be a square multiple of its
/// 15-pixel vanilla size; regions without a matching sprite stay empty.
pub(crate) fn icons_from_sprites(sprites: &BTreeMap<String, RgbaImage>) -> Option<RgbaImage> {
    let (width, height) = sprites.get("crosshair")?.dimensions();
    if width != height || width == 0 || !width.is_multiple_of(15) {
        return None;
    }
    let scale = width / 15;
    let mut atlas = canvas(256 * scale, 256 * scale)?;
    for (name, region) in LEGACY_ICON_RECTS {
        let Some(sprite) = sprites.get(name) else {
            continue;
        };
        if sprite.dimensions() == (region.w * scale, region.h * scale) {
            paste(&mut atlas, sprite, region.x * scale, region.y * scale);
        }
    }
    Some(atlas)
}
