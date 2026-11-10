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
    texture: impl FnMut(&str) -> Option<&'a EquipmentTexture>,
) -> Option<EquipmentTexture> {
    layers(
        armor_material,
        material,
        pattern,
        slot,
        icon,
        overrides,
        texture,
    )?
    .compose(base, dye)
}

/// The validated mask and palettes one trim draws with, shared by composition and measurement.
pub struct TrimLayers<'a> {
    mask: &'a EquipmentTexture,
    palette: &'a EquipmentTexture,
    target: &'a EquipmentTexture,
}

/// Resolves the trim's mask and material palettes. `None` when any is missing or malformed,
/// which is exactly when [`compose`] keeps the undecorated fallback for a valid base.
pub fn layers<'a>(
    armor_material: Option<ArmourMaterial>,
    material: &str,
    pattern: &str,
    slot: ArmorSlot,
    icon: bool,
    overrides: Option<&crate::equipment::EquipmentResources>,
    mut texture: impl FnMut(&str) -> Option<&'a EquipmentTexture>,
) -> Option<TrimLayers<'a>> {
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
    if [mask, palette, target]
        .into_iter()
        .any(|image| !valid(image))
        || palette.rgba8.len() != target.rgba8.len()
    {
        return None;
    }
    Some(TrimLayers {
        mask,
        palette,
        target,
    })
}

/// Whether an image has a nonzero size and exactly its RGBA8 texels.
fn valid(image: &EquipmentTexture) -> bool {
    image.width != 0
        && image.height != 0
        && image.rgba8.len() == usize::from(image.width) * usize::from(image.height) * 4
}

/// The source texel coordinate nearest-sampled for output coordinate `at` along one axis.
fn source_coordinate(at: u16, source: u16, output: u16) -> usize {
    usize::from(at) * usize::from(source) / usize::from(output)
}

/// Straight-alpha source-over coverage for a trim texel of alpha `over` above `under`.
fn coverage(over: u8, under: u8) -> u32 {
    let over = u32::from(over);
    over * 255 + u32::from(under) * (255 - over)
}

impl TrimLayers<'_> {
    /// The composed size over a base of `base` width and height.
    pub fn size(&self, base: [u16; 2]) -> [u16; 2] {
        [base[0].max(self.mask.width), base[1].max(self.mask.height)]
    }

    /// The composed alpha at `[x, y]` of [`Self::size`] over an undyed base of size `base`,
    /// where `under` reads the base's alpha at a base texel. Matches [`Self::compose`] without
    /// copying pixels.
    pub fn alpha(&self, base: [u16; 2], under: impl Fn(usize, usize) -> u8, x: u16, y: u16) -> u8 {
        let [width, height] = self.size(base);
        let under = under(
            source_coordinate(x, base[0], width),
            source_coordinate(y, base[1], height),
        );
        let over = self.mask.rgba8[(source_coordinate(y, self.mask.height, height)
            * usize::from(self.mask.width)
            + source_coordinate(x, self.mask.width, width))
            * 4
            + 3];
        ((coverage(over, under) + 127) / 255) as u8
    }

    /// Composes the trim over `base`, applying `dye` to the base only.
    pub fn compose(&self, base: &EquipmentTexture, dye: Option<Dye>) -> Option<EquipmentTexture> {
        if !valid(base) {
            return None;
        }
        let [width, height] = self.size([base.width, base.height]);
        let sample = |image: &EquipmentTexture, x: u16, y: u16| -> [u8; 4] {
            let sx = source_coordinate(x, image.width, width);
            let sy = source_coordinate(y, image.height, height);
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
                let mut over = sample(self.mask, x, y);
                if let Some(index) = self
                    .palette
                    .rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .position(|key| key[..3] == over[..3])
                {
                    over[..3].copy_from_slice(&self.target.rgba8[index * 4..index * 4 + 3]);
                }
                let a = u32::from(over[3]);
                // Straight-alpha source-over retains transparent holes in item and armor sheets.
                let alpha = coverage(over[3], under[3]);
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
