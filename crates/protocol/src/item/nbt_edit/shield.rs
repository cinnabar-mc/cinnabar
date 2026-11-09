//! Shield decoration copies the complete banner data while retaining the shield's state.
use super::*;

/// Applies one banner to an undecorated shield, preserving shield tags and its wire trailer.
/// A shield with a base tag or a banner with more than six overlays is ineligible.
pub fn apply_banner_to_shield(shield: &[u8], banner: &[u8], color: u32) -> Option<Arc<[u8]>> {
    let (root, mut shield, trailer) = read_extra_entries(shield)?;
    if shield.iter().any(|entry| entry.name == b"Base") {
        return None;
    }
    let (_, banner, _) = read_extra_entries(banner)?;
    if let Some(patterns) = banner
        .iter()
        .find(|entry| entry.name == b"Patterns" && entry.tag == TAG_LIST)
    {
        let count = i32::from_le_bytes(patterns.payload.get(1..5)?.try_into().ok()?);
        if count > 6 {
            return None;
        }
    }
    let ominous = banner.iter().any(|entry| {
        entry.name == b"Type" && entry.tag == TAG_INT && entry.payload == 1i32.to_le_bytes()
    });
    if ominous {
        let mut fields = Vec::new();
        let mut pattern = Vec::new();
        write_string(&mut pattern, b"ill")?;
        set_entry(&mut fields, b"Pattern", Some((TAG_STRING, pattern)));
        set_entry(
            &mut fields,
            b"Color",
            Some((TAG_INT, 15i32.to_le_bytes().to_vec())),
        );
        let mut list = vec![TAG_COMPOUND];
        list.extend(1i32.to_le_bytes());
        write_entries(&mut list, &fields)?;
        set_entry(&mut shield, b"Patterns", Some((TAG_LIST, list)));
    } else {
        for entry in banner {
            set_entry(&mut shield, &entry.name, Some((entry.tag, entry.payload)));
        }
    }
    set_entry(
        &mut shield,
        b"Base",
        Some((TAG_INT, i32::from(color as i16).to_le_bytes().to_vec())),
    );
    let mut out = vec![255, 255, 1, TAG_COMPOUND];
    write_string(&mut out, &root)?;
    write_entries(&mut out, &shield)?;
    out.extend_from_slice(trailer);
    super::super::super::inventory::validate_item_nbt_size(out.len()).ok()?;
    Some(out.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes an original banner fixture without introducing any game assets.
    fn banner(layers: usize, ominous: bool) -> Vec<u8> {
        let mut fields = Vec::new();
        if ominous {
            set_entry(
                &mut fields,
                b"Type",
                Some((TAG_INT, 1i32.to_le_bytes().to_vec())),
            );
        }
        let mut layer = vec![TAG_STRING];
        write_string(&mut layer, b"Pattern").unwrap();
        write_string(&mut layer, b"sc").unwrap();
        layer.push(TAG_INT);
        write_string(&mut layer, b"Color").unwrap();
        layer.extend(4i32.to_le_bytes());
        layer.push(0);
        let mut patterns = vec![TAG_COMPOUND];
        patterns.extend((layers as i32).to_le_bytes());
        for _ in 0..layers {
            patterns.extend_from_slice(&layer);
        }
        set_entry(&mut fields, b"Patterns", Some((TAG_LIST, patterns)));
        let mut out = vec![255, 255, 1, TAG_COMPOUND, 0, 0];
        write_entries(&mut out, &fields).unwrap();
        out.extend([0; 8]);
        out
    }

    #[test]
    fn shield_decoration_preserves_damage_name_and_blocking_tick() {
        let mut source = vec![0; 18];
        source[10..].copy_from_slice(&42i64.to_le_bytes());
        let original = edit_item_extra(
            &source,
            &ItemNbtEdit {
                damage: Some(7),
                custom_name: Some(Some("Old".into())),
                ..Default::default()
            },
        )
        .unwrap();
        let decorated = apply_banner_to_shield(&original, &banner(2, false), 15).unwrap();
        assert_eq!(crate::item_extra_damage(&decorated), Some(7));
        assert_eq!(crate::item_custom_name(&decorated).as_deref(), Some("Old"));
        assert_eq!(
            decorated[decorated.len() - 16..],
            original[original.len() - 16..]
        );
        assert_eq!(crate::item_banner_patterns(&decorated).len(), 2);
        assert!(apply_banner_to_shield(&decorated, &banner(1, false), 0).is_none());
        assert!(apply_banner_to_shield(&original, &banner(7, false), 0).is_none());
        assert!(apply_banner_to_shield(&original, &banner(6, false), 0).is_some());
    }

    #[test]
    fn ominous_banner_uses_its_special_single_overlay() {
        let decorated = apply_banner_to_shield(&[0; 18], &banner(2, true), 15).unwrap();
        let patterns = crate::item_banner_patterns(&decorated);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern.as_ref(), "ill");
        assert_eq!(patterns[0].color, 15);
    }
}
