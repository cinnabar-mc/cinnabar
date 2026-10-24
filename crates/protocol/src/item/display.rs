//! The player-visible strings and enchantments an item's user data carries.

use std::sync::Arc;

use super::{decode_extra_nbt, read_i32_le, read_u8, read_u16_le, root_tag, skip_le_payload};

/// Bounds keep a hostile stack from growing tooltip work without limit.
const MAX_LORE_LINES: usize = 32;
const MAX_ENCHANTMENTS: usize = 32;
const MAX_TEXT_BYTES: usize = 1024;

const TAG_SHORT: u8 = 2;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;

/// What a tooltip reads from a stack's user data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDisplay {
    /// The `display.Name` an anvil or command set.
    pub name: Option<Arc<str>>,
    pub lore: Vec<Arc<str>>,
    /// `(enchantment id, level)` from the root `ench` list.
    pub enchantments: Vec<(i16, u8)>,
}

/// Reads a stack's custom name, lore and enchantments; malformed data reads as absent.
#[must_use]
pub fn item_display(extra_data: &[u8]) -> ItemDisplay {
    let mut display = ItemDisplay::default();
    let Some(nbt) = decode_extra_nbt(extra_data) else {
        return display;
    };
    let mut cursor = nbt;
    if let Some(payload) = root_tag(&mut cursor, TAG_COMPOUND, b"display") {
        read_display(payload, &mut display);
    }
    let mut cursor = nbt;
    if let Some(payload) = root_tag(&mut cursor, TAG_LIST, b"ench") {
        read_enchantments(payload, &mut display);
    }
    display
}

const TAG_INT: u8 = 3;
/// Pages a book keeps.
pub const MAX_BOOK_PAGES: usize = 50;

/// A written or writable book's text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemBook {
    pub pages: Vec<Arc<str>>,
    pub title: Option<Arc<str>>,
    pub author: Option<Arc<str>>,
}

/// Reads a book's pages, title and author from its user data.
#[must_use]
pub fn item_book(extra_data: &[u8]) -> ItemBook {
    let mut book = ItemBook::default();
    let Some(nbt) = decode_extra_nbt(extra_data) else {
        return book;
    };
    for (name, target) in [(&b"title"[..], 0), (&b"author"[..], 1)] {
        let mut cursor = nbt;
        if let Some(mut payload) = root_tag(&mut cursor, TAG_STRING, name)
            && let Some(text) = read_text(&mut payload)
        {
            if target == 0 {
                book.title = Some(text);
            } else {
                book.author = Some(text);
            }
        }
    }
    let mut cursor = nbt;
    if let Some(payload) = root_tag(&mut cursor, TAG_LIST, b"pages") {
        read_pages(payload, &mut book);
    }
    book
}

fn read_pages(mut list: &[u8], book: &mut ItemBook) {
    let _ = (|| -> Option<()> {
        if read_u8(&mut list)? != TAG_COMPOUND {
            return None;
        }
        let count = usize::try_from(read_i32_le(&mut list)?).ok()?;
        for _ in 0..count {
            let mut text = None;
            loop {
                let tag = read_u8(&mut list)?;
                if tag == 0 {
                    break;
                }
                let name_len = usize::from(read_u16_le(&mut list)?);
                let name = list.get(..name_len)?;
                list = list.get(name_len..)?;
                if tag == TAG_STRING && name == b"text" {
                    text = Some(read_text(&mut list)?);
                } else {
                    skip_le_payload(&mut list, tag, 1)?;
                }
            }
            if book.pages.len() < MAX_BOOK_PAGES {
                book.pages.push(text.unwrap_or_else(|| Arc::from("")));
            }
        }
        Some(())
    })();
}

/// Whether the stack's user data carries an `ench` list, which glints an item even when empty.
#[must_use]
pub fn item_has_enchantment_list(extra_data: &[u8]) -> bool {
    decode_extra_nbt(extra_data).is_some_and(|nbt| {
        let mut cursor = nbt;
        root_tag(&mut cursor, TAG_LIST, b"ench").is_some()
    })
}

/// The key vanilla's UI binds as `#item_id_aux` for a stack: the network id in the high 16 bits,
/// the auxiliary value in the low 16, with bit 15 set when the user data carries an `ench` list.
/// Humanoid armour (`humanoid_armour`, a damageable vanilla armour item) uses the root integer
/// `Damage` of its user data as the auxiliary value, or 0 without one, instead of the wire metadata.
/// An empty stack reads as -1, which matches no item.
#[must_use]
pub fn item_id_aux(stack: &super::NetworkItemStack, humanoid_armour: bool) -> i32 {
    if stack.is_empty() {
        return -1;
    }
    let enchanted = if item_has_enchantment_list(&stack.extra_data) {
        0x8000
    } else {
        0
    };
    // The client widens the 16-bit auxiliary value with its sign before combining it.
    let aux = if humanoid_armour {
        super::item_extra_damage_value(&stack.extra_data).unwrap_or(0)
    } else {
        i32::from(stack.metadata as u16 as i16)
    };
    ((u32::from(stack.network_id as u16) << 16) as i32 | aux) | enchanted
}

/// Reads the `bundle_id` int that ties a bundle item to its dynamic container.
#[must_use]
pub fn item_bundle_id(extra_data: &[u8]) -> Option<u32> {
    let nbt = decode_extra_nbt(extra_data)?;
    let mut cursor = nbt;
    let mut payload = root_tag(&mut cursor, TAG_INT, b"bundle_id")?;
    u32::try_from(read_i32_le(&mut payload)?).ok()
}

fn read_text(cursor: &mut &[u8]) -> Option<Arc<str>> {
    let length = usize::from(read_u16_le(cursor)?);
    let bytes = cursor.get(..length)?;
    *cursor = cursor.get(length..)?;
    let bytes = &bytes[..bytes.len().min(MAX_TEXT_BYTES)];
    Some(Arc::from(String::from_utf8_lossy(bytes).as_ref()))
}

fn read_display(mut payload: &[u8], display: &mut ItemDisplay) {
    let _ = (|| -> Option<()> {
        loop {
            let tag = read_u8(&mut payload)?;
            if tag == 0 {
                return Some(());
            }
            let name_len = usize::from(read_u16_le(&mut payload)?);
            let name = payload.get(..name_len)?;
            payload = payload.get(name_len..)?;
            match (tag, name) {
                (TAG_STRING, b"Name") => display.name = Some(read_text(&mut payload)?),
                (TAG_LIST, b"Lore") => {
                    if read_u8(&mut payload)? != TAG_STRING {
                        return None;
                    }
                    let count = usize::try_from(read_i32_le(&mut payload)?).ok()?;
                    for _ in 0..count {
                        let line = read_text(&mut payload)?;
                        if display.lore.len() < MAX_LORE_LINES {
                            display.lore.push(line);
                        }
                    }
                }
                _ => skip_le_payload(&mut payload, tag, 1)?,
            }
        }
    })();
}

fn read_enchantments(mut list: &[u8], display: &mut ItemDisplay) {
    let _ = (|| -> Option<()> {
        if read_u8(&mut list)? != TAG_COMPOUND {
            return None;
        }
        let count = usize::try_from(read_i32_le(&mut list)?).ok()?;
        for _ in 0..count {
            let (mut id, mut level) = (None, None);
            loop {
                let tag = read_u8(&mut list)?;
                if tag == 0 {
                    break;
                }
                let name_len = usize::from(read_u16_le(&mut list)?);
                let name = list.get(..name_len)?;
                list = list.get(name_len..)?;
                if tag == TAG_SHORT && (name == b"id" || name == b"lvl") {
                    let value = i16::from_le_bytes(list.get(..2)?.try_into().ok()?);
                    *if name == b"id" { &mut id } else { &mut level } = Some(value);
                }
                skip_le_payload(&mut list, tag, 1)?;
            }
            if let (Some(id), Some(level)) = (id, level)
                && display.enchantments.len() < MAX_ENCHANTMENTS
            {
                display
                    .enchantments
                    .push((id, u8::try_from(level).unwrap_or(u8::MAX)));
            }
        }
        Some(())
    })();
}

/// The enchantments a stack holds as bits of a mask, read from its whole root `ench` list
/// without the tooltip cap. As vanilla's enchantment check does, an id counts only when its
/// first entry has a positive level. `None` when the user data, root compound or list cannot
/// be read in full, an entry lacks its id or level, or an id falls outside the mask, so a
/// clear bit always means that enchantment is absent.
#[must_use]
pub fn item_enchantment_mask(extra_data: &[u8]) -> Option<u64> {
    let Some(header) = extra_data.get(..2) else {
        return extra_data.is_empty().then_some(0);
    };
    match i16::from_le_bytes([header[0], header[1]]) {
        // A zero length carries no user-data compound.
        0 => return Some(0),
        -1 => {}
        _ => return None,
    }
    let mut cursor = decode_extra_nbt(extra_data)?;
    if read_u8(&mut cursor)? != TAG_COMPOUND {
        return None;
    }
    let name_len = usize::from(read_u16_le(&mut cursor)?);
    cursor = cursor.get(name_len..)?;
    loop {
        let tag = read_u8(&mut cursor)?;
        if tag == 0 {
            return Some(0);
        }
        let name_len = usize::from(read_u16_le(&mut cursor)?);
        let name = cursor.get(..name_len)?;
        cursor = cursor.get(name_len..)?;
        if tag == TAG_LIST && name == b"ench" {
            return enchantment_list_mask(cursor);
        }
        skip_le_payload(&mut cursor, tag, 0)?;
    }
}

/// Folds each id's first entry by its level; an empty list of any element type holds none.
fn enchantment_list_mask(mut list: &[u8]) -> Option<u64> {
    let element = read_u8(&mut list)?;
    let count = read_i32_le(&mut list)?;
    if count <= 0 {
        return Some(0);
    }
    if element != TAG_COMPOUND {
        return None;
    }
    let (mut seen, mut mask) = (0_u64, 0_u64);
    for _ in 0..count {
        let (mut id, mut level) = (None, None);
        loop {
            let tag = read_u8(&mut list)?;
            if tag == 0 {
                break;
            }
            let name_len = usize::from(read_u16_le(&mut list)?);
            let name = list.get(..name_len)?;
            list = list.get(name_len..)?;
            if tag == TAG_SHORT && (name == b"id" || name == b"lvl") {
                let value = i16::from_le_bytes(list.get(..2)?.try_into().ok()?);
                *if name == b"id" { &mut id } else { &mut level } = Some(value);
            }
            skip_le_payload(&mut list, tag, 1)?;
        }
        let bit = 1_u64.checked_shl(u32::try_from(id?).ok()?)?;
        if seen & bit == 0 && level? > 0 {
            mask |= bit;
        }
        seen |= bit;
    }
    Some(mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_tag(out: &mut Vec<u8>, name: &str, value: &str) {
        out.push(TAG_STRING);
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(name.as_bytes());
        out.extend((value.len() as u16).to_le_bytes());
        out.extend(value.as_bytes());
    }

    fn extra(build: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut nbt = vec![TAG_COMPOUND, 0, 0];
        build(&mut nbt);
        nbt.push(0);
        let mut extra = (-1_i16).to_le_bytes().to_vec();
        extra.push(1);
        extra.extend(nbt);
        extra
    }

    #[test]
    fn damaged_humanoid_armour_keys_its_damage_instead_of_metadata() {
        let mut stack = super::super::NetworkItemStack {
            network_id: 350,
            metadata: 0,
            count: 1,
            ..Default::default()
        };
        assert_eq!(item_id_aux(&stack, true), 350 << 16);
        stack.extra_data = extra(|nbt| {
            nbt.push(3);
            nbt.extend(6_u16.to_le_bytes());
            nbt.extend(b"Damage");
            nbt.extend(7_i32.to_le_bytes());
        })
        .into();
        assert_eq!(item_id_aux(&stack, true), (350 << 16) | 7);
        stack.metadata = 2;
        assert_eq!(item_id_aux(&stack, false), (350 << 16) | 2);
        assert_eq!(item_id_aux(&stack, true), (350 << 16) | 7);
    }

    #[test]
    fn id_aux_packs_id_aux_and_enchantment_list() {
        let mut stack = super::super::NetworkItemStack {
            network_id: 448,
            metadata: 3,
            count: 1,
            ..Default::default()
        };
        assert_eq!(item_id_aux(&stack, false), (448 << 16) | 3);
        stack.extra_data = extra(|nbt| {
            nbt.push(TAG_LIST);
            nbt.extend(4_u16.to_le_bytes());
            nbt.extend(b"ench");
            nbt.push(TAG_COMPOUND);
            nbt.extend(0_i32.to_le_bytes());
        })
        .into();
        assert_eq!(item_id_aux(&stack, false), (448 << 16) | 0x8000 | 3);
        stack.count = 0;
        assert_eq!(item_id_aux(&stack, false), -1);
        assert_eq!(
            item_id_aux(&super::super::NetworkItemStack::empty(), false),
            -1
        );
    }

    #[test]
    fn id_aux_sign_extends_metadata_with_bit_15_set() {
        let mut stack = super::super::NetworkItemStack {
            network_id: 448,
            metadata: 0x8000,
            count: 1,
            ..Default::default()
        };
        assert_eq!(
            item_id_aux(&stack, false),
            (448 << 16) | i32::from(i16::MIN)
        );
        stack.metadata = 0xffff;
        assert_eq!(item_id_aux(&stack, false), -1);
        stack.metadata = 0x7fff;
        assert_eq!(item_id_aux(&stack, false), (448 << 16) | 0x7fff);
    }

    #[test]
    fn reads_name_lore_and_enchantments() {
        let data = extra(|nbt| {
            nbt.push(TAG_COMPOUND);
            nbt.extend(7_u16.to_le_bytes());
            nbt.extend(b"display");
            string_tag(nbt, "Name", "Blade");
            nbt.push(TAG_LIST);
            nbt.extend(4_u16.to_le_bytes());
            nbt.extend(b"Lore");
            nbt.push(TAG_STRING);
            nbt.extend(2_i32.to_le_bytes());
            for line in ["one", "two"] {
                nbt.extend((line.len() as u16).to_le_bytes());
                nbt.extend(line.as_bytes());
            }
            nbt.push(0);
            nbt.push(TAG_LIST);
            nbt.extend(4_u16.to_le_bytes());
            nbt.extend(b"ench");
            nbt.push(TAG_COMPOUND);
            nbt.extend(1_i32.to_le_bytes());
            for (key, value) in [("id", 9_i16), ("lvl", 3)] {
                nbt.push(TAG_SHORT);
                nbt.extend((key.len() as u16).to_le_bytes());
                nbt.extend(key.as_bytes());
                nbt.extend(value.to_le_bytes());
            }
            nbt.push(0);
        });
        assert!(item_has_enchantment_list(&data));
        let display = item_display(&data);
        assert_eq!(display.name.as_deref(), Some("Blade"));
        assert_eq!(display.lore.len(), 2);
        assert_eq!(display.enchantments, vec![(9, 3)]);
    }

    #[test]
    fn reads_book_pages_and_title() {
        let data = extra(|nbt| {
            string_tag(nbt, "title", "Log");
            nbt.push(TAG_LIST);
            nbt.extend(5_u16.to_le_bytes());
            nbt.extend(b"pages");
            nbt.push(TAG_COMPOUND);
            nbt.extend(2_i32.to_le_bytes());
            for page in ["one", "two"] {
                string_tag(nbt, "text", page);
                nbt.push(0);
            }
        });
        let book = item_book(&data);
        assert_eq!(book.title.as_deref(), Some("Log"));
        assert_eq!(book.pages.len(), 2);
        assert_eq!(&*book.pages[1], "two");
    }

    #[test]
    fn reads_the_bundle_id() {
        let data = extra(|nbt| {
            nbt.push(TAG_INT);
            nbt.extend(9_u16.to_le_bytes());
            nbt.extend(b"bundle_id");
            nbt.extend(42_i32.to_le_bytes());
        });
        assert_eq!(item_bundle_id(&data), Some(42));
        assert_eq!(item_bundle_id(&[]), None);
        assert!(!item_has_enchantment_list(&data));
    }

    #[test]
    fn malformed_data_reads_as_absent() {
        assert_eq!(item_display(&[1, 2, 3]), ItemDisplay::default());
        assert_eq!(item_display(&[]), ItemDisplay::default());
    }

    /// Appends one `ench` entry holding only its id and level 1.
    fn enchantment_entry(nbt: &mut Vec<u8>, id: i16) {
        leveled_entry(nbt, id, 1);
    }

    /// Appends one `ench` entry holding only its id and level.
    fn leveled_entry(nbt: &mut Vec<u8>, id: i16, level: i16) {
        for (key, value) in [("id", id), ("lvl", level)] {
            nbt.push(TAG_SHORT);
            nbt.extend((key.len() as u16).to_le_bytes());
            nbt.extend(key.as_bytes());
            nbt.extend(value.to_le_bytes());
        }
        nbt.push(0);
    }

    #[test]
    fn the_enchantment_mask_reads_entries_beyond_the_tooltip_cap() {
        let fire_aspect = 13;
        let data = extra(|nbt| {
            nbt.push(TAG_LIST);
            nbt.extend(4_u16.to_le_bytes());
            nbt.extend(b"ench");
            nbt.push(TAG_COMPOUND);
            let count = MAX_ENCHANTMENTS as i16 + 1;
            nbt.extend(i32::from(count).to_le_bytes());
            for _ in 0..MAX_ENCHANTMENTS {
                enchantment_entry(nbt, 0);
            }
            enchantment_entry(nbt, fire_aspect);
        });
        assert!(
            item_display(&data)
                .enchantments
                .iter()
                .all(|(id, _)| *id != fire_aspect)
        );
        assert_eq!(item_enchantment_mask(&data), Some(1 | 1 << fire_aspect));
    }

    #[test]
    fn the_enchantment_mask_is_unknown_unless_the_whole_list_reads() {
        assert_eq!(item_enchantment_mask(&[]), Some(0));
        assert_eq!(item_enchantment_mask(&0_i16.to_le_bytes()), Some(0));
        assert_eq!(item_enchantment_mask(&extra(|_| {})), Some(0));
        let mut truncated = extra(|nbt| {
            nbt.push(TAG_LIST);
            nbt.extend(4_u16.to_le_bytes());
            nbt.extend(b"ench");
            nbt.push(TAG_COMPOUND);
            nbt.extend(1_i32.to_le_bytes());
            enchantment_entry(nbt, 9);
        });
        assert_eq!(item_enchantment_mask(&truncated), Some(1 << 9));
        truncated.truncate(truncated.len() - 6);
        assert_eq!(item_enchantment_mask(&truncated), None);
        let out_of_range = extra(|nbt| {
            nbt.push(TAG_LIST);
            nbt.extend(4_u16.to_le_bytes());
            nbt.extend(b"ench");
            nbt.push(TAG_COMPOUND);
            nbt.extend(1_i32.to_le_bytes());
            enchantment_entry(nbt, 64);
        });
        assert_eq!(item_enchantment_mask(&out_of_range), None);
        assert_eq!(item_enchantment_mask(&5_i16.to_le_bytes()), None);
    }

    #[test]
    fn an_enchantment_counts_only_when_its_first_entry_has_a_positive_level() {
        let list = |entries: &[(i16, i16)]| {
            extra(|nbt| {
                nbt.push(TAG_LIST);
                nbt.extend(4_u16.to_le_bytes());
                nbt.extend(b"ench");
                nbt.push(TAG_COMPOUND);
                nbt.extend((entries.len() as i32).to_le_bytes());
                for (id, level) in entries {
                    leveled_entry(nbt, *id, *level);
                }
            })
        };
        assert_eq!(item_enchantment_mask(&list(&[(13, 0)])), Some(0));
        assert_eq!(item_enchantment_mask(&list(&[(13, -1)])), Some(0));
        assert_eq!(item_enchantment_mask(&list(&[(13, 0), (13, 2)])), Some(0));
        assert_eq!(
            item_enchantment_mask(&list(&[(13, 2), (13, 0)])),
            Some(1 << 13)
        );
    }
}
