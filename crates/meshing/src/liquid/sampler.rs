//! Mesh-job liquid and occlusion memoization.

use super::*;

const HALO_SIDE: usize = SIDE + 2;
const HALO_VOLUME: usize = HALO_SIDE * HALO_SIDE * HALO_SIDE;
const NO_SOURCE: u16 = 1 << 15;
const LIQUID_KNOWN: u16 = 1 << 14;
const LIQUID_PRESENT: u16 = 1 << 13;
const OCCLUSION_KNOWN: u16 = 1 << 12;
const OCCLUDES: u16 = 1 << 11;
const FULL_CELL_BARRIER: u16 = 1 << 10;

/// Facts and liquid reference for one halo cell, read together on cache hits.
#[derive(Clone, Copy, Default)]
struct MemoSlot {
    flags: u16,
    /// One plus the index into `Sampler::cells`, or zero for no supported liquid.
    cell_ref: u8,
}

/// Owns one mesh job's independently filled liquid and occlusion facts.
pub(super) struct Sampler<'chunk, 'assets> {
    resolvers: [Option<ContributorResolver<'chunk>>; 27],
    assets: &'assets RuntimeAssets,
    slots: Box<[MemoSlot]>,
    cells: Vec<LiquidCell>,
}

impl<'chunk, 'assets> Sampler<'chunk, 'assets> {
    /// Prepares palette resolvers and empty halo slots for one mesh job.
    pub(super) fn new(
        classifier: BlockClassifier,
        assets: &'assets RuntimeAssets,
        mode: NetworkIdMode,
        neighbourhood: &MeshNeighbourhood<'chunk>,
    ) -> Self {
        let mut resolvers = std::array::from_fn(|_| None);
        for (offset, chunk) in neighbourhood.liquid_sub_chunks() {
            if let Some(chunk) = chunk {
                resolvers[offset_index(offset)] =
                    Some(ContributorResolver::new(classifier, assets, mode, chunk));
            }
        }
        Self {
            resolvers,
            assets,
            slots: vec![MemoSlot::default(); HALO_VOLUME].into_boxed_slice(),
            cells: Vec::new(),
        }
    }

    /// Resolves a coordinate from the captured neighbourhood, including non-halo cells.
    fn contributors(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<ResolvedContributors> {
        let (_, local) = neighbourhood.liquid_block_source(coordinate)?;
        let offset = coordinate
            .map(|value| i8::try_from(value.div_euclid(16)).ok())
            .map(Option::unwrap);
        self.resolvers[offset_index(offset)]
            .as_ref()
            .map(|resolver| resolver.resolve(local))
    }

    /// Fills the requested part once; a full liquid table falls back to direct resolution.
    fn slot(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        index: usize,
        coordinate: [i32; 3],
        want: u16,
    ) -> Option<MemoSlot> {
        let mut slot = self.slots[index];
        let flags = slot.flags;
        if flags & (want | NO_SOURCE) != 0 {
            return Some(slot);
        }
        let Some(contributors) = self.contributors(neighbourhood, coordinate) else {
            slot.flags = NO_SOURCE;
            self.slots[index] = slot;
            return Some(slot);
        };
        let mut updated = flags | want;
        if want == LIQUID_KNOWN {
            let part = LiquidPart::resolve(self.assets, contributors);
            if let Some(cell) = part.cell {
                let cells = &mut self.cells;
                let position = match cells.iter().position(|known| *known == cell) {
                    Some(position) => position,
                    None if cells.len() < usize::from(u8::MAX) => {
                        cells.push(cell);
                        cells.len() - 1
                    }
                    None => return None,
                };
                slot.cell_ref = position as u8 + 1;
            }
            if part.present {
                updated |= LIQUID_PRESENT;
            }
        } else {
            let part = OcclusionPart::resolve(self.assets, contributors);
            updated |= u16::from(part.opaque_faces);
            if part.occludes {
                updated |= OCCLUDES;
            }
            if part.full_cell_barrier {
                updated |= FULL_CELL_BARRIER;
            }
        }
        slot.flags = updated;
        self.slots[index] = slot;
        Some(slot)
    }

    /// Whether the primary block at `coordinate` is one of the sorted `water_filled` values.
    pub(super) fn water_filled(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
        water_filled: &[u32],
    ) -> bool {
        !water_filled.is_empty()
            && self
                .contributors(neighbourhood, coordinate)
                .and_then(ResolvedContributors::primary_network_value)
                .is_some_and(|value| water_filled.binary_search(&value).is_ok())
    }
}

impl LiquidSampler for Sampler<'_, '_> {
    fn liquid_part(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<LiquidPart> {
        let slot = halo_index(coordinate)
            .and_then(|index| self.slot(neighbourhood, index, coordinate, LIQUID_KNOWN));
        let Some(slot) = slot else {
            return self
                .contributors(neighbourhood, coordinate)
                .map(|contributors| LiquidPart::resolve(self.assets, contributors));
        };
        let flags = slot.flags;
        (flags & NO_SOURCE == 0).then(|| LiquidPart {
            present: flags & LIQUID_PRESENT != 0,
            cell: usize::from(slot.cell_ref)
                .checked_sub(1)
                .map(|position| self.cells[position]),
        })
    }

    fn occlusion_part(
        &mut self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<OcclusionPart> {
        let Some(index) = halo_index(coordinate) else {
            return self
                .contributors(neighbourhood, coordinate)
                .map(|contributors| OcclusionPart::resolve(self.assets, contributors));
        };
        let flags = self
            .slot(neighbourhood, index, coordinate, OCCLUSION_KNOWN)?
            .flags;
        (flags & NO_SOURCE == 0).then_some(OcclusionPart {
            occludes: flags & OCCLUDES != 0,
            full_cell_barrier: flags & FULL_CELL_BARRIER != 0,
            opaque_faces: flags as u8,
        })
    }
}

/// Maps the one-cell meshing halo to a slot; wider snapshot samples use the direct path.
fn halo_index(coordinate: [i32; 3]) -> Option<usize> {
    let [x, y, z] = coordinate.map(|value| {
        usize::try_from(value.wrapping_add(1))
            .ok()
            .filter(|&value| value < HALO_SIDE)
    });
    Some((x? * HALO_SIDE + y?) * HALO_SIDE + z?)
}

/// Selects the resolver for one of the captured 27 sub-chunk offsets.
const fn offset_index([x, y, z]: [i8; 3]) -> usize {
    ((x + 1) as usize) * 9 + ((y + 1) as usize) * 3 + (z + 1) as usize
}
