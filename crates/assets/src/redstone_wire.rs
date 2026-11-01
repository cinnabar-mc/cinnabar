//! Power colour carried by redstone wire materials.
//!
//! Wire shares the stem material's fixed colour channel: the signal sits in material bits only leaf
//! materials otherwise use, and those can never carry the channel's flag.

use crate::compiled::MATERIAL_LEAF_AO_EXPONENT_SHIFT;

/// Bit offset of the encoded signal; zero means the material is a stem, not wire.
pub const SIGNAL_SHIFT: u32 = MATERIAL_LEAF_AO_EXPONENT_SHIFT;
/// Material bits holding the signal plus one.
pub const SIGNAL_MASK: u32 = 0x1f << SIGNAL_SHIFT;
/// The strongest power a wire carries.
pub const MAX_SIGNAL: u32 = 15;

/// Encodes a wire's power without changing any other material facts.
#[must_use]
pub const fn material_flags(signal: u32) -> Option<u32> {
    if signal <= MAX_SIGNAL {
        Some(crate::stem::MATERIAL_FLAG | (signal + 1) << SIGNAL_SHIFT)
    } else {
        None
    }
}

/// Returns the power's RGB vertex tint; it is independent of the biome.
#[must_use]
pub fn colour(signal: u32) -> Option<[u8; 3]> {
    if signal > MAX_SIGNAL {
        return None;
    }
    let power = signal as f32 / MAX_SIGNAL as f32;
    let red = if signal == 0 { 0.3 } else { 0.6 * power + 0.4 };
    let green = 0.7 * power * power - 0.5;
    let blue = 0.6 * power * power - 0.7;
    // Truncating, saturating float casts match the native conversion to the byte range.
    Some([red, green, blue].map(|channel: f32| (channel * 255.0) as u8))
}

/// Whether `flags` describe a wire rather than a stem, which share the fixed colour flag.
#[must_use]
pub const fn is_wire_material(flags: u32) -> bool {
    flags & crate::stem::MATERIAL_FLAG != 0 && encoded_signal(flags).is_some()
}

/// Resolves the encoded wire power of a material carrying the fixed colour flag.
#[must_use]
pub(crate) const fn encoded_signal(flags: u32) -> Option<u32> {
    match (flags & SIGNAL_MASK) >> SIGNAL_SHIFT {
        0 => None,
        encoded => Some(encoded - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_colour_runs_from_dim_to_bright_red() {
        assert_eq!(colour(0), Some([76, 0, 0]));
        assert_eq!(colour(15), Some([255, 50, 0]));
        assert_eq!(colour(16), None);
        assert_eq!(material_flags(16), None);
        assert!(is_wire_material(material_flags(0).unwrap()));
        assert!(!is_wire_material(crate::stem::material_flags(7).unwrap()));
        assert!(!is_wire_material(SIGNAL_MASK));
        let reds = (0..=MAX_SIGNAL).map(|signal| colour(signal).unwrap()[0]);
        assert!(reds.clone().zip(reds.skip(1)).all(|(low, high)| low < high));
    }

    #[test]
    fn every_power_decodes_through_the_fixed_colour_channel() {
        for signal in 0..=MAX_SIGNAL {
            let flags = material_flags(signal).unwrap() | crate::MATERIAL_FLAG_ALPHA_CUTOUT;
            assert_eq!(crate::stem::material_colour(flags), colour(signal));
            assert!(crate::compiled::material_flags_are_valid(flags));
        }
        let stem = crate::stem::material_flags(3).unwrap();
        assert_eq!(crate::stem::material_colour(stem), crate::stem::colour(3));
        assert!(!crate::compiled::material_flags_are_valid(
            stem | 1 << SIGNAL_SHIFT
        ));
    }
}
