//! Bedrock's root Trim compound, independent of item names and display tags.
use super::*;

/// Material and pattern identifiers stored on one trimmed stack.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemTrim {
    pub material: Arc<str>,
    pub pattern: Arc<str>,
}

/// Reads both case-sensitive strings from the root Trim compound.
#[must_use]
pub fn item_trim(extra_data: &[u8]) -> Option<ItemTrim> {
    let mut root = decode_extra_nbt(extra_data)?;
    let trim = root_tag(&mut root, TAG_COMPOUND, b"Trim")?;
    let string = |name: &[u8]| {
        let mut cursor = trim;
        let mut payload = compound_field(&mut cursor, TAG_STRING, name)?;
        let length = usize::from(read_u16_le(&mut payload)?);
        let text = std::str::from_utf8(payload.get(..length)?).ok()?;
        (!text.is_empty()).then(|| Arc::from(text))
    };
    Some(ItemTrim {
        material: string(b"Material")?,
        pattern: string(b"Pattern")?,
    })
}

/// Replaces only Trim, preserving damage, enchantments, custom data and the wire trailer.
pub fn apply_item_trim(extra_data: &[u8], trim: &ItemTrim) -> Option<Arc<[u8]>> {
    if trim.material.is_empty() || trim.pattern.is_empty() {
        return None;
    }
    let (root, mut entries, trailer) = read_extra_entries(extra_data)?;
    let mut fields = Vec::new();
    for (name, value) in [
        (b"Material".as_slice(), &trim.material),
        (b"Pattern".as_slice(), &trim.pattern),
    ] {
        let mut payload = Vec::new();
        write_string(&mut payload, value.as_bytes())?;
        set_entry(&mut fields, name, Some((TAG_STRING, payload)));
    }
    let mut compound = Vec::new();
    write_entries(&mut compound, &fields)?;
    set_entry(&mut entries, b"Trim", Some((TAG_COMPOUND, compound)));
    let mut out = vec![255, 255, 1, TAG_COMPOUND];
    write_string(&mut out, &root)?;
    write_entries(&mut out, &entries)?;
    out.extend_from_slice(trailer);
    crate::inventory::validate_item_nbt_size(out.len()).ok()?;
    Some(out.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_and_retrimming_keep_the_base_stack_data() {
        let original = edit_item_extra(
            &[],
            &ItemNbtEdit {
                damage: Some(17),
                custom_name: Some(Some("Keepsake".into())),
                enchantments: Some(vec![(0, 4)]),
                ..Default::default()
            },
        )
        .unwrap();
        for material in ["gold", "netherite"] {
            let trim = ItemTrim {
                material: material.into(),
                pattern: "sentry".into(),
            };
            let trimmed = apply_item_trim(&original, &trim).unwrap();
            assert_eq!(item_trim(&trimmed), Some(trim.clone()));
            assert_eq!(crate::item_extra_damage(&trimmed), Some(17));
            assert_eq!(item_anvil_data(&trimmed), item_anvil_data(&original));
            assert!(trimmed.ends_with(&original[original.len() - 8..]));
            let replaced = apply_item_trim(
                &trimmed,
                &ItemTrim {
                    pattern: "coast".into(),
                    ..trim
                },
            )
            .unwrap();
            assert_eq!(item_trim(&replaced).unwrap().pattern.as_ref(), "coast");
        }
        assert_eq!(item_trim(&original), None);
        assert_eq!(item_trim(&[]), None);
        assert!(
            apply_item_trim(
                &[5, 0],
                &ItemTrim {
                    material: "gold".into(),
                    pattern: "sentry".into()
                }
            )
            .is_none()
        );
    }
}
