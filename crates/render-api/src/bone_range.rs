//! Bounds checks for published bone-matrix ranges.

/// A capped serializable prefix and the validity of the complete published range.
#[derive(Debug)]
pub struct BoneRange<'a> {
    /// Matrices available for output after the caller's cap, if the range fits the arena.
    pub matrices: Option<&'a [[[f32; 4]; 3]]>,
    /// Whether every bone in the full range is present, regardless of the output cap.
    pub full_range_valid: bool,
}

/// Reads a capped prefix of a published bone arena and independently validates the full range.
#[must_use]
pub fn bone_range(
    arena: &[[[f32; 4]; 3]],
    base: u32,
    count: usize,
    full_count: usize,
) -> BoneRange<'_> {
    let Some(start) = usize::try_from(base).ok() else {
        return BoneRange {
            matrices: None,
            full_range_valid: false,
        };
    };
    let prefix = start
        .checked_add(count)
        .and_then(|end| arena.get(start..end));
    let full = start
        .checked_add(full_count)
        .and_then(|end| arena.get(start..end));
    BoneRange {
        matrices: prefix,
        full_range_valid: full.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::bone_range;

    #[test]
    fn full_bone_range_validity_is_not_hidden_by_output_cap() {
        let bone = [[0.0_f32; 4]; 3];
        let arena = vec![bone; 4_096];
        let cases = [
            // Capped output hides an arena that cannot hold the full pose.
            ("truncated prefix", 0, 4_096, 8_192, true, false),
            // Control: a complete valid pose stays valid.
            ("complete range", 0, 4_096, 4_096, true, true),
            ("base past arena", 8_192, 4_096, 8_192, false, false),
            ("empty full range", 4_096, 0, 0, true, true),
        ];
        for (name, base, count, full_count, prefix_valid, full_valid) in cases {
            let range = bone_range(&arena, base, count, full_count);
            assert_eq!(range.matrices.is_some(), prefix_valid, "{name}: prefix");
            assert_eq!(range.full_range_valid, full_valid, "{name}: full range");
        }
    }
}
