//! Conditional terrain markers: barrier, light-block and structure-void cells.
//!
//! Vanilla tessellates these blocks only into their own terrain layers, which it draws while the
//! local player meets each layer's condition. Meshing records the cells instead of geometry, so
//! the renderer can show or hide a layer without remeshing.

use std::sync::Arc;

use assets::{TerrainMarker, VisualKind};

use crate::contributors::{PaletteFacts, ResolvedPaletteEntry};
use crate::lighting::{LightingInputs, MeshLightSample};

const CLASS_SHIFT: u32 = 12;
const LIGHT_SHIFT: u32 = 16;

/// One marker cell of a sub-chunk: its local position, layer, block and own-cell light.
///
/// The block is kept as its sequential ID rather than a material, so the renderer resolves the
/// marker's art from whichever pack is current without remeshing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PackedTerrainMarker {
    cell_class_light: u32,
    block: u32,
}

impl PackedTerrainMarker {
    /// Packs `local` cell coordinates below 16, the marker class, its sequential block ID and
    /// light.
    #[must_use]
    pub const fn new(
        local: [u8; 3],
        marker: TerrainMarker,
        block: u32,
        light: MeshLightSample,
    ) -> Self {
        Self {
            cell_class_light: (local[0] as u32 & 15)
                | ((local[1] as u32 & 15) << 4)
                | ((local[2] as u32 & 15) << 8)
                | ((marker.variant() & 3) << CLASS_SHIFT)
                | ((light.block() as u32 | ((light.sky() as u32) << 4)) << LIGHT_SHIFT),
            block,
        }
    }

    /// The cell's x, y and z inside its sub-chunk.
    #[must_use]
    pub const fn local(self) -> [u8; 3] {
        [
            (self.cell_class_light & 15) as u8,
            ((self.cell_class_light >> 4) & 15) as u8,
            ((self.cell_class_light >> 8) & 15) as u8,
        ]
    }

    /// The marker's layer class.
    #[must_use]
    pub fn marker(self) -> TerrainMarker {
        TerrainMarker::from_variant(
            VisualKind::Invisible,
            (self.cell_class_light >> CLASS_SHIFT) & 3,
        )
        .expect("packed markers always hold a class")
    }

    /// The sequential ID of the marker's block.
    #[must_use]
    pub const fn block(self) -> u32 {
        self.block
    }

    /// The cell's solved light, block level in the low nibble and sky level in the high nibble.
    #[must_use]
    pub const fn light(self) -> u8 {
        (self.cell_class_light >> LIGHT_SHIFT) as u8
    }
}

/// The marker class and sequential block ID of an entry, if it is a known marker block.
fn entry_marker(entry: &ResolvedPaletteEntry) -> Option<(TerrainMarker, u32)> {
    TerrainMarker::from_variant(entry.kind, entry.variant).zip(entry.sequential_id)
}

/// Records every marker cell of the section with the light solved at that cell.
pub(crate) fn collect(
    facts: &PaletteFacts<'_>,
    lighting: &impl LightingInputs,
) -> Arc<[PackedTerrainMarker]> {
    if !facts.has_terrain_markers() {
        return Arc::from([]);
    }
    let mut markers = Vec::new();
    facts.for_each_voxel_class(entry_marker, |x, y, z, class| {
        if let Some((marker, block)) = class {
            let light = lighting.sample([x as i32, y as i32, z as i32]);
            markers.push(PackedTerrainMarker::new(
                [x as u8, y as u8, z as u8],
                marker,
                block,
                light,
            ));
        }
    });
    Arc::from(markers)
}
