//! Trim palette substitution over the active pack's item and armor artwork.
use crate::{ArmorSlot, EquipmentTexture, facts::ArmourMaterial};
use std::sync::Arc;
#[cfg(test)]
mod tests;

pub const TEXTURE_DIRECTORY: &str = "textures/trims/";

/// Whether base alpha represents leather cutout coverage or retained texture opacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DyeAlpha {
    Coverage,
    Preserve,
}

/// A color-mask multiplier and the material's interpretation of base alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dye {
    pub rgb: [u8; 3],
    pub alpha: DyeAlpha,
}

/// The native item mask name follows the admitted armor slot.
pub const fn slot_name(slot: ArmorSlot) -> &'static str {
    match slot {
        ArmorSlot::Helmet => "helmet",
        ArmorSlot::Chestplate => "chestplate",
        ArmorSlot::Leggings => "leggings",
        ArmorSlot::Boots => "boots",
    }
}

/// Composes a native trim without changing unrelated texels or tinting the trim with leather dye.
/// Missing patterns or palettes retain the caller's undecorated fallback.
#[allow(clippy::too_many_arguments)]
pub fn compose<'a>(
    base: &EquipmentTexture,
    armor_material: Option<ArmourMaterial>,
    material: &str,
    pattern: &str,
    slot: ArmorSlot,
    icon: bool,
    dye: Option<Dye>,
    overrides: Option<&crate::equipment::EquipmentResources>,
    mut texture: impl FnMut(&str) -> Option<&'a EquipmentTexture>,
) -> Option<EquipmentTexture> {
    let material = material.strip_prefix("minecraft:").unwrap_or(material);
    let pattern = pattern.strip_prefix("minecraft:").unwrap_or(pattern);
    let mask = if icon {
        texture(&format!(
            "{TEXTURE_DIRECTORY}items/{}_trim",
            slot_name(slot)
        ))?
    } else {
        let suffix = if slot == ArmorSlot::Leggings {
            "_leggings"
        } else {
            ""
        };
        let native = format!("{TEXTURE_DIRECTORY}{pattern}{suffix}");
        let authored = overrides
            .and_then(|resources| resources.textures.get(format!("{pattern}_trim").as_str()));
        texture(authored.map_or(native.as_str(), AsRef::as_ref))?
    };
    let palette = texture(&format!("{TEXTURE_DIRECTORY}color_palettes/trim_palette"))?;
    let darker = armor_material.is_some_and(|armor| material == armor.trim_material());
    let target_path = format!(
        "{TEXTURE_DIRECTORY}color_palettes/{material}{}",
        if darker { "_darker" } else { "" }
    );
    let target_path = overrides
        .and_then(|resources| {
            resources
                .textures
                .get(format!("{material}_palette").as_str())
        })
        .map_or(target_path.as_str(), AsRef::as_ref);
    let target = texture(target_path)
        .or_else(|| texture(&format!("{TEXTURE_DIRECTORY}color_palettes/{material}")))?;
    for image in [base, mask, palette, target] {
        if image.width == 0
            || image.height == 0
            || image.rgba8.len() != usize::from(image.width) * usize::from(image.height) * 4
        {
            return None;
        }
    }
    if palette.rgba8.len() != target.rgba8.len() {
        return None;
    }
    let (width, height) = (base.width.max(mask.width), base.height.max(mask.height));
    let sample = |image: &EquipmentTexture, x: u16, y: u16| -> [u8; 4] {
        let sx = usize::from(x) * usize::from(image.width) / usize::from(width);
        let sy = usize::from(y) * usize::from(image.height) / usize::from(height);
        image.rgba8[(sy * usize::from(image.width) + sx) * 4..][..4]
            .try_into()
            .unwrap()
    };
    let mut pixels = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
    for y in 0..height {
        for x in 0..width {
            let mut under = sample(base, x, y);
            if let Some(dye) = dye {
                let alpha = under[3];
                under = crate::color_mask_texel(under, dye.rgb);
                if dye.alpha == DyeAlpha::Preserve {
                    under[3] = alpha;
                }
            }
            let mut over = sample(mask, x, y);
            if let Some(index) = palette
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .position(|key| key[..3] == over[..3])
            {
                over[..3].copy_from_slice(&target.rgba8[index * 4..index * 4 + 3]);
            }
            let a = u32::from(over[3]);
            // Straight-alpha source-over retains transparent holes in item and armor sheets.
            let alpha = a * 255 + u32::from(under[3]) * (255 - a);
            let mut result = [0; 4];
            for c in 0..3 {
                result[c] = (u32::from(over[c]) * a * 255
                    + u32::from(under[c]) * u32::from(under[3]) * (255 - a)
                    + alpha / 2)
                    .checked_div(alpha)
                    .unwrap_or(0) as u8;
            }
            result[3] = ((alpha + 127) / 255) as u8;
            pixels.extend(result);
        }
    }
    Some(EquipmentTexture {
        identifier: base.identifier.clone(),
        width,
        height,
        rgba8: Arc::from(pixels),
    })
}

/// Identifies only the four native trimmable armor slots from pinned item facts.
pub fn armor_slot(identifier: &str) -> Option<ArmorSlot> {
    use super::armour::{ItemArmour, ItemArmourSlot};
    match super::armour::builtin_item_armour(identifier) {
        ItemArmour::Wearable { slot, .. } => Some(match slot {
            ItemArmourSlot::Head => ArmorSlot::Helmet,
            ItemArmourSlot::Chest => ArmorSlot::Chestplate,
            ItemArmourSlot::Legs => ArmorSlot::Leggings,
            ItemArmourSlot::Feet => ArmorSlot::Boots,
        }),
        _ => None,
    }
}
