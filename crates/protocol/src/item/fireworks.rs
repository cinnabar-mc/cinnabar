//! Firework rocket and firework star user data, as their item hover text reads it.

use super::{decode_extra_nbt, read_i32_le, read_u8, read_u16_le, root_tag, skip_le_payload};

const TAG_BYTE: u8 = 1;
const TAG_BYTE_ARRAY: u8 = 7;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;

/// One firework explosion: a star's `FireworksItem` compound or a rocket's `Explosions` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FireworkExplosion {
    /// `FireworkType`; an absent or non-byte tag reads as 0, the small ball.
    pub shape: u8,
    /// `FireworkColor` dye colour indices, in stored order.
    pub colors: Vec<u8>,
    /// `FireworkFade` dye colour indices, in stored order.
    pub fade_colors: Vec<u8>,
    /// A non-zero `FireworkTrail` byte.
    pub trail: bool,
    /// A non-zero `FireworkFlicker` byte.
    pub flicker: bool,
}

/// A firework rocket's root `Fireworks` compound.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemFireworks {
    /// The `Flight` byte, when the compound carries one.
    pub flight: Option<u8>,
    /// Each compound of the `Explosions` list, in stored order.
    pub explosions: Vec<FireworkExplosion>,
}

/// Reads a rocket's `Fireworks` compound; `None` without one or when it is malformed.
#[must_use]
pub fn item_fireworks(extra_data: &[u8]) -> Option<ItemFireworks> {
    let mut cursor = decode_extra_nbt(extra_data)?;
    let mut payload = root_tag(&mut cursor, TAG_COMPOUND, b"Fireworks")?;
    let mut fireworks = ItemFireworks::default();
    loop {
        let tag = read_u8(&mut payload)?;
        if tag == 0 {
            return Some(fireworks);
        }
        let name = read_name(&mut payload)?;
        match (tag, name) {
            (TAG_BYTE, b"Flight") => fireworks.flight = Some(read_u8(&mut payload)?),
            (TAG_LIST, b"Explosions") => read_explosions(&mut payload, &mut fireworks)?,
            _ => skip_le_payload(&mut payload, tag, 1)?,
        }
    }
}

/// Reads a firework star's root `FireworksItem` compound; `None` without one or when malformed.
#[must_use]
pub fn item_firework_star(extra_data: &[u8]) -> Option<FireworkExplosion> {
    let mut cursor = decode_extra_nbt(extra_data)?;
    let mut payload = root_tag(&mut cursor, TAG_COMPOUND, b"FireworksItem")?;
    read_explosion(&mut payload, 1)
}

/// Reads the name of the tag whose type byte was just consumed.
fn read_name<'a>(cursor: &mut &'a [u8]) -> Option<&'a [u8]> {
    let length = usize::from(read_u16_le(cursor)?);
    let name = cursor.get(..length)?;
    *cursor = cursor.get(length..)?;
    Some(name)
}

/// Reads an `Explosions` list payload; entries of another tag type are skipped.
fn read_explosions(cursor: &mut &[u8], fireworks: &mut ItemFireworks) -> Option<()> {
    let element = read_u8(cursor)?;
    let count = usize::try_from(read_i32_le(cursor)?).ok()?;
    for _ in 0..count {
        if element == TAG_COMPOUND {
            fireworks.explosions.push(read_explosion(cursor, 2)?);
        } else {
            skip_le_payload(cursor, element, 2)?;
        }
    }
    Some(())
}

/// Reads one explosion compound body through its end tag.
fn read_explosion(cursor: &mut &[u8], depth: u32) -> Option<FireworkExplosion> {
    let mut explosion = FireworkExplosion::default();
    loop {
        let tag = read_u8(cursor)?;
        if tag == 0 {
            return Some(explosion);
        }
        let name = read_name(cursor)?;
        match (tag, name) {
            (TAG_BYTE, b"FireworkType") => explosion.shape = read_u8(cursor)?,
            (TAG_BYTE, b"FireworkTrail") => explosion.trail = read_u8(cursor)? != 0,
            (TAG_BYTE, b"FireworkFlicker") => explosion.flicker = read_u8(cursor)? != 0,
            (TAG_BYTE_ARRAY, b"FireworkColor") => explosion.colors = read_bytes(cursor)?,
            (TAG_BYTE_ARRAY, b"FireworkFade") => explosion.fade_colors = read_bytes(cursor)?,
            _ => skip_le_payload(cursor, tag, depth)?,
        }
    }
}

/// Reads a byte-array payload without trusting its length beyond the remaining data.
fn read_bytes(cursor: &mut &[u8]) -> Option<Vec<u8>> {
    let length = usize::try_from(read_i32_le(cursor)?).ok()?;
    let bytes = cursor.get(..length)?.to_vec();
    *cursor = cursor.get(length..)?;
    Some(bytes)
}

#[cfg(test)]
mod tests;
