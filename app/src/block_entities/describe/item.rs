//! Contained item identity and decoration retained for fixed world placements.
use super::*;

/// Reads a usable retained item stack.
pub(super) fn held_item(stack: &NbtCompound) -> Option<HeldItem> {
    let name = stack.string("Name").filter(|name| !name.is_empty())?;
    Some(HeldItem {
        identifier: Arc::from(name),
        shield_pattern: (name == assets::gui_item::SHIELD_IDENTIFIER)
            .then(|| shield_pattern(stack.compound("tag")?))
            .flatten()
            .map(Arc::new),
        metadata: stack
            .integer("Damage")
            .and_then(|damage| u32::try_from(damage).ok())
            .unwrap_or(0),
    })
}

/// Reads shield cloth data from a block entity's typed item tags.
fn shield_pattern(tag: &NbtCompound) -> Option<render_api::ShieldPattern> {
    let base = match tag.get("Base") {
        Some(world::NbtValue::Int(base)) => Some(*base),
        _ => None,
    };
    let layers = tag
        .list("Patterns")
        .unwrap_or_default()
        .iter()
        .filter_map(|layer| {
            let world::NbtValue::Compound(layer) = layer else {
                return None;
            };
            Some(render_api::BannerPattern {
                pattern: layer.string("Pattern").unwrap_or_default().into(),
                color: match layer.get("Color") {
                    Some(world::NbtValue::Int(color)) => *color as u8,
                    _ => 0,
                },
            })
        });
    render_api::ShieldPattern::new(base, layers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use world::NbtValue;

    #[test]
    fn framed_shield_retains_ordered_cloth_layers() {
        let mut layer = NbtCompound::default();
        layer.insert("Pattern", NbtValue::String("sc".into()));
        layer.insert("Color", NbtValue::Int(260));
        let mut tag = NbtCompound::default();
        tag.insert("Base", NbtValue::Int(-1));
        tag.insert(
            "Patterns",
            NbtValue::List(vec![NbtValue::Compound(layer); 8]),
        );
        let mut item = NbtCompound::default();
        item.insert(
            "Name",
            NbtValue::String(assets::gui_item::SHIELD_IDENTIFIER.into()),
        );
        item.insert("tag", NbtValue::Compound(tag));
        let appearance = held_item(&item).unwrap().shield_pattern.unwrap();
        assert_eq!(appearance.base, 15);
        assert_eq!(appearance.layers.len(), render_api::SHIELD_PATTERN_LAYERS);
        assert!(
            appearance
                .layers
                .iter()
                .all(|layer| layer.pattern.as_ref() == "sc" && layer.color == 4)
        );
        item.insert("Name", NbtValue::String("minecraft:stone".into()));
        assert!(held_item(&item).unwrap().shield_pattern.is_none());
    }
}
