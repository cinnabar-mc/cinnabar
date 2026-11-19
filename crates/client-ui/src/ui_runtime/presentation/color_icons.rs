//! Per-stack dye masks reuse admitted raw artwork without allocating color variants.

use super::UiPresentationRuntime;
use std::collections::BTreeMap;
use ui::IconRef;

pub(super) type MaskRefs = BTreeMap<Box<str>, BTreeMap<u32, IconRef>>;

/// Binds mask-enabled items to the raw source region retained in their icon carrier.
pub(super) fn prepare(
    entities: &assets::RuntimeEntityAssets,
    icons: &assets::RuntimeIconCatalog,
    refs: &[IconRef],
) -> MaskRefs {
    let mut out = MaskRefs::new();
    let Some(sources) = icons.sources() else {
        return out;
    };
    for item in entities.item_visuals() {
        if assets::facts::pack::rules()
            .default_item_tint(&item.key.identifier)
            .is_none()
            && !assets::facts::pack::leather_item(&item.key.identifier)
        {
            continue;
        }
        let assets::ItemVisualDefinitionRoute::Sprite { texture } = item.route else {
            continue;
        };
        let Some(source) = entities.sources().get(texture.source as usize) else {
            continue;
        };
        if let Some(icon) = sources
            .iter()
            .find(|raw| raw.path.eq_ignore_ascii_case(&source.path))
            .and_then(|raw| refs.get(raw.sprite as usize))
        {
            out.entry(item.key.identifier.clone())
                .or_default()
                .insert(item.key.metadata, *icon);
        }
    }
    out
}

impl UiPresentationRuntime {
    /// Resolves a stack's dyed artwork: its own dye, else the default leather colour for a
    /// leather item whose icon comes from a pack. `None` leaves the plain icon to draw.
    pub(super) fn dyed_item_icon(
        &self,
        identifier: &str,
        metadata: u32,
        dye: Option<u32>,
    ) -> Option<IconRef> {
        let rgb = dye.or_else(|| {
            (assets::facts::pack::leather_item(identifier)
                && self.session_item_icon(identifier, metadata).is_some())
            .then_some(assets::DEFAULT_LEATHER_RGB)
        })?;
        self.color_mask_icon(identifier, metadata, rgb)
    }

    /// Resolves a stack's RGB mask while preserving session icon overrides and zero-variant fallback.
    /// A pack's leather artwork is grey and relies on this tint, so it is the one override
    /// that is masked.
    pub(super) fn color_mask_icon(
        &self,
        identifier: &str,
        metadata: u32,
        rgb: u32,
    ) -> Option<IconRef> {
        if let Some(icon) = self.session_item_icon(identifier, metadata) {
            return assets::facts::pack::leather_item(identifier).then_some(IconRef {
                color_mask: Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]),
                ..icon
            });
        }
        let variants = self.gui_models.sources.color_masks.get(identifier)?;
        let icon = variants.get(&metadata).or_else(|| variants.get(&0))?;
        Some(IconRef {
            color_mask: Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]),
            ..*icon
        })
    }
}

#[cfg(test)]
mod tests;
