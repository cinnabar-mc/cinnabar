//! The cell overlays a mesh job draws: the overlay hosts of its sub-chunk, captured with the job's
//! snapshot on the stream thread and resolved on the worker.

use super::super::*;

/// The `CinnabarOverlays` block entities of one sub-chunk at dispatch, by cell, with the registry
/// view that resolves their block states. Empty for the sub-chunks that carry none, which capture
/// no registry view.
#[derive(Default)]
pub(in crate::stream) struct OverlayHosts {
    hosts: Vec<([u8; 3], Arc<world::CellOverlays>)>,
    ids: Option<DecodeIds>,
}

impl WorldStream {
    /// The overlay hosts in `key`, found by the column's block entity range for the sub-chunk.
    pub(in crate::stream) fn overlay_hosts(&self, key: SubChunkKey) -> OverlayHosts {
        let Some(chunk) = self.authority.terrain().chunk(key.chunk()) else {
            return OverlayHosts::default();
        };
        let side = world::SUB_CHUNK_SIDE as i32;
        let hosts: Vec<_> = chunk
            .sub_chunk_block_entities(key)
            .filter_map(|(entity, nbt)| {
                let overlays = nbt
                    .cell_overlays()
                    .filter(|overlays| !overlays.is_empty())?;
                let local = entity.position().map(|axis| axis.rem_euclid(side) as u8);
                Some((local, Arc::clone(overlays)))
            })
            .collect();
        let ids = (!hosts.is_empty()).then(|| self.authority.decode_ids(key.dimension));
        OverlayHosts { hosts, ids }
    }
}

impl OverlayHosts {
    /// Overlays the hosts hold before resolution: the most the job can draw.
    pub(in crate::stream) fn count(&self) -> usize {
        self.hosts.iter().map(|(_, overlays)| overlays.len()).sum()
    }

    /// The block-state overlays of every host whose cell in `center` still holds its host state,
    /// in cell and slot order. Item overlays are drawn with block entities, not in the mesh.
    pub(in crate::stream) fn resolve(&self, center: &SubChunk) -> Vec<::meshing::CellOverlay> {
        let Some(ids) = &self.ids else {
            return Vec::new();
        };
        let mut overlays = Vec::new();
        for (local, host) in &self.hosts {
            let resolved = host.resolve(ids);
            let Some(backing) = center.runtime_id(0, local[0], local[1], local[2]) else {
                continue;
            };
            let Some(on) = resolved.on(backing) else {
                continue;
            };
            overlays.extend(on.iter().filter_map(|overlay| match overlay.look {
                world::OverlayLook::Block(network_value) => Some(::meshing::CellOverlay {
                    local: *local,
                    face: overlay.face,
                    network_value,
                }),
                world::OverlayLook::Item(_) => None,
            }));
        }
        overlays
    }
}
