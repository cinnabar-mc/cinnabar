//! Checked publication of direct-occlusion records and their readback ring.

use super::super::model::CullRecord;
use super::*;

impl DirectOcclusion {
    /// Keeps old storage on failure without publishing an incomplete table.
    pub(super) fn upload(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Result<(), crate::gpu_allocation::TerrainWriteError> {
        let slots = self.table.slot_count();
        if self
            .storage
            .as_ref()
            .is_none_or(|storage| storage.capacity < slots)
        {
            let Some(capacity) = OcclusionStorage::capacity(&device.limits(), slots) else {
                return Err(crate::gpu_allocation::TerrainWriteError);
            };
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!(
                "terrain.occlusion_allocate",
                capacity,
                readback_bytes = occlusion_bytes(capacity) * SLOTS as u64
            )
            .entered();
            let Some((storage, readbacks)) =
                self.failed.attempt(Entity::PLACEHOLDER, capacity, || {
                    crate::gpu_allocation::terrain_checked(
                        device,
                        "terrain occlusion storage",
                        u64::from(capacity) * size_of::<CullRecord>() as u64,
                        || occlusion_buffers(device.wgpu_device(), capacity),
                    )
                })
            else {
                return Err(crate::gpu_allocation::TerrainWriteError);
            };
            self.verdicts.reset();
            self.bind_group = None;
            self.storage = Some(storage);
            self.readbacks = readbacks;
            self.table.mark_all_dirty();
        }
        let storage = self.storage.as_ref().expect("storage was just ensured");
        let uploaded = write_dirty_records(&mut self.table, device, queue, &storage.records);
        if uploaded.is_err() {
            self.table.mark_all_dirty();
        }
        uploaded
    }
}

/// Allocates occlusion storage and its readback ring as one transaction.
fn occlusion_buffers(
    device: &wgpu::Device,
    capacity: u32,
) -> (OcclusionStorage, Vec<wgpu::Buffer>) {
    let readbacks = (0..SLOTS)
        .map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("terrain occlusion readback"),
                size: occlusion_bytes(capacity),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .collect();
    (OcclusionStorage::new(device, capacity), readbacks)
}
