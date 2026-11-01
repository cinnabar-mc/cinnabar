//! Immutable growth colour carried by stem materials.

/// Identifies a stem whose tint depends on its encoded growth stage.
pub const MATERIAL_FLAG: u32 = 1 << 28;
/// Bit offset of the bounded three-bit growth stage.
pub const GROWTH_SHIFT: u32 = 29;
/// Material bits reserved for stem growth.
pub const GROWTH_MASK: u32 = 7 << GROWTH_SHIFT;

/// Encodes a supported growth stage without changing any other material facts.
#[must_use]
pub const fn material_flags(growth: u32) -> Option<u32> {
    if growth <= 7 {
        Some(MATERIAL_FLAG | growth << GROWTH_SHIFT)
    } else {
        None
    }
}

/// Returns the stage's RGB vertex tint; it is independent of the biome.
#[must_use]
pub const fn colour(growth: u32) -> Option<[u8; 3]> {
    if growth <= 7 {
        Some([
            (growth * 32) as u8,
            (255 - growth * 8) as u8,
            (growth * 4) as u8,
        ])
    } else {
        None
    }
}

/// Resolves the encoded stem or redstone wire colour, leaving ordinary materials untinted.
#[must_use]
pub fn material_colour(flags: u32) -> Option<[u8; 3]> {
    if flags & MATERIAL_FLAG == 0 {
        return None;
    }
    match crate::redstone_wire::encoded_signal(flags) {
        Some(signal) => crate::redstone_wire::colour(signal),
        None => colour((flags & GROWTH_MASK) >> GROWTH_SHIFT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_colour_is_independent_of_other_material_flags() {
        assert_eq!(colour(0), Some([0, 255, 0]));
        assert_eq!(colour(7), Some([224, 199, 28]));
        assert_eq!(colour(8), None);
        assert_eq!(material_flags(8), None);
        for growth in 0..=7 {
            let flags = material_flags(growth).unwrap() | crate::MATERIAL_FLAG_ALPHA_CUTOUT;
            assert_eq!(material_colour(flags), colour(growth));
            assert!(crate::compiled::material_flags_are_valid(flags));
        }
        assert_eq!(material_colour(crate::MATERIAL_FLAG_GRASS_TINT), None);
        assert!(!crate::compiled::material_flags_are_valid(GROWTH_MASK));
        assert!(!crate::compiled::material_flags_are_valid(
            MATERIAL_FLAG | crate::MATERIAL_FLAG_GRASS_TINT
        ));
    }
}
