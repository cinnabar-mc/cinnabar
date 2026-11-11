//! Trim support a component item declares: the `minecraft:trimmable_armors` tag, and the
//! `minecraft:icon` textures that replace its icon's trim mask or a material's palette.

use std::{collections::BTreeMap, sync::Arc};

use crate::nbt_tree::Nbt;

/// The item tag that lets a component item take armor trims.
pub const TRIMMABLE_ARMORS_TAG: &str = "minecraft:trimmable_armors";
/// The icon texture whose item atlas image replaces the slot's item trim mask.
const ICON_MASK_TEXTURE: &str = "icon_trim";
/// Icon textures whose name contains this marker name a material's palette by the text before it.
const PALETTE_MARKER: &str = "_palette";

/// The icon trim artwork a trimmable component item declares beside its default icon.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CustomArmorTrim {
    /// `textures.icon_trim`: an item atlas key whose image replaces the slot's item trim mask.
    pub icon_mask: Option<Arc<str>>,
    /// Palette texture paths by material name, from `textures.<material>_palette`.
    pub icon_palettes: BTreeMap<Arc<str>, Arc<str>>,
}

impl CustomArmorTrim {
    /// The palette texture path the icon declares for `material` (without its namespace).
    pub fn icon_palette(&self, material: &str) -> Option<&str> {
        self.icon_palettes.get(material).map(AsRef::as_ref)
    }

    /// Counts the owned text for admission budgets.
    pub(super) fn retained_bytes(&self) -> usize {
        self.icon_palettes
            .iter()
            .map(|(material, path)| material.len() + path.len() + 64)
            .chain(self.icon_mask.as_deref().map(|mask| mask.len() + 32))
            .sum()
    }
}

/// Reads the icon trim artwork of an item whose `tags` include [`TRIMMABLE_ARMORS_TAG`]; `None`
/// for any other item. As the reference client does, every icon texture named
/// `<material>_palette…` declares that material's palette, and the first declaration of a
/// material wins.
pub(super) fn declared(components: Nbt<'_>, tags: &[Arc<str>]) -> Option<CustomArmorTrim> {
    if !tags.iter().any(|tag| tag.as_ref() == TRIMMABLE_ARMORS_TAG) {
        return None;
    }
    let mut trim = CustomArmorTrim::default();
    let textures = components
        .field("minecraft:icon")
        .or_else(|| components.field("item_properties")?.field("minecraft:icon"))
        .and_then(|icon| icon.field("textures"));
    let Some(Nbt::Compound(textures)) = textures else {
        return Some(trim);
    };
    let text = |value: Nbt<'_>| {
        value
            .as_str()
            .filter(|text| !text.is_empty() && text.len() <= super::components::MAX_TEXT_BYTES)
            .map(Arc::<str>::from)
    };
    for (name, value) in textures.iter() {
        if name == ICON_MASK_TEXTURE {
            trim.icon_mask = trim.icon_mask.or_else(|| text(value));
        } else if let Some(at) = name.find(PALETTE_MARKER)
            && let Some(path) = text(value)
        {
            trim.icon_palettes.entry(name[..at].into()).or_insert(path);
        }
    }
    Some(trim)
}
