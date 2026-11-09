//! Deduplicates the halo cells a block ray inspects.

use std::collections::BTreeSet;

use super::precision::FloatRay;

/// Largest bitset, in cells, one ray allocates; a longer ray records its cells in the ordered set.
const MAX_DENSE_CELLS: usize = 1 << 16;

/// Cells a ray traversal has already inspected.
///
/// Neighbouring traversal cells share almost all of their halo, so each cell is checked many
/// times. Cells inside the halo-expanded bounds of the ray's segment use a bitset; any other
/// cell falls back to an ordered set, so membership never depends on the bounds being exact.
pub(super) struct InspectedCells {
    min: [i64; 3],
    dims: [usize; 3],
    bits: Vec<u64>,
    outside: BTreeSet<[i32; 3]>,
}

impl InspectedCells {
    /// Sizes the bitset to cover every cell within `halo` of the cells `ray` crosses.
    pub(super) fn new(ray: FloatRay, halo: [(i32, i32); 3]) -> Self {
        let mut min = [0; 3];
        let mut dims = [0; 3];
        for axis in 0..3 {
            let start = f64::from(ray.origin[axis]);
            let end = start + f64::from(ray.delta[axis]);
            if !start.is_finite() || !end.is_finite() {
                return Self::ordered();
            }
            // A cell of slack on each side covers boundary ties and endpoint rounding.
            let low = start.min(end).floor() as i64 - 1 + i64::from(halo[axis].0);
            let high = start.max(end).floor() as i64 + 1 + i64::from(halo[axis].1);
            min[axis] = low;
            dims[axis] = usize::try_from(high - low + 1).unwrap_or(0);
        }
        match dims
            .iter()
            .try_fold(1_usize, |cells, &dim| cells.checked_mul(dim))
        {
            Some(cells) if cells <= MAX_DENSE_CELLS => Self {
                min,
                dims,
                bits: vec![0; cells.div_ceil(64)],
                outside: BTreeSet::new(),
            },
            _ => Self::ordered(),
        }
    }

    /// Records every cell in the ordered set.
    fn ordered() -> Self {
        Self {
            min: [0; 3],
            dims: [0; 3],
            bits: Vec::new(),
            outside: BTreeSet::new(),
        }
    }

    /// Marks `cell` inspected, returning whether it was not inspected before.
    pub(super) fn insert(&mut self, cell: [i32; 3]) -> bool {
        let mut index = 0_usize;
        for axis in 0..3 {
            let offset = usize::try_from(i64::from(cell[axis]) - self.min[axis]);
            match offset {
                Ok(offset) if offset < self.dims[axis] => index = index * self.dims[axis] + offset,
                _ => return self.outside.insert(cell),
            }
        }
        let (word, bit) = (index / 64, 1_u64 << (index % 64));
        let fresh = self.bits[word] & bit == 0;
        self.bits[word] |= bit;
        fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vec3;

    /// Bitset and fallback cells together dedupe exactly like one ordered set.
    #[test]
    fn matches_an_ordered_set_inside_and_outside_the_bounds() {
        let ray = FloatRay::new(Vec3::new(0.5, 64.2, -3.7), Vec3::new(0.6, -0.48, 0.64), 5.7);
        let halo = [(-2, 2), (-2, 2), (-2, 2)];
        let mut inspected = InspectedCells::new(ray, halo);
        assert!(!inspected.bits.is_empty());
        let mut reference = BTreeSet::new();
        for x in -12..12 {
            for y in 52..76 {
                for z in -16..8 {
                    for cell in [[x, y, z], [x, y, z]] {
                        assert_eq!(inspected.insert(cell), reference.insert(cell), "{cell:?}");
                    }
                }
            }
        }
        assert!(!inspected.outside.is_empty());
    }

    /// Rays too long for the bitset still dedupe through the ordered set.
    #[test]
    fn long_rays_use_the_ordered_set() {
        let ray = FloatRay::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.6, 0.0, 0.8), 1.0e6);
        let mut inspected = InspectedCells::new(ray, [(-2, 2); 3]);
        assert!(inspected.bits.is_empty());
        assert!(inspected.insert([1, 2, 3]));
        assert!(!inspected.insert([1, 2, 3]));
    }
}
