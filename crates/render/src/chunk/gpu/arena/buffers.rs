//! Checked indirect-command storage for terrain draws.

use crate::chunk::*;

/// `None` when the device is out of memory for it.
pub(in crate::chunk) fn create_indirect_buffer(
    render_device: &RenderDevice,
    command_capacity: usize,
) -> Option<Buffer> {
    crate::gpu_allocation::terrain_buffer(
        render_device,
        &indirect_buffer_descriptor(command_capacity),
    )
}

/// Describes the shared indexed indirect-command buffer at its requested capacity.
pub(in crate::chunk::gpu) fn indirect_buffer_descriptor(
    command_capacity: usize,
) -> BufferDescriptor<'static> {
    BufferDescriptor {
        label: Some("packed chunk indirect commands"),
        size: command_capacity as u64 * INDEXED_INDIRECT_BYTES,
        usage: BufferUsages::INDIRECT
            | BufferUsages::COPY_DST
            | if cfg!(test) {
                BufferUsages::COPY_SRC
            } else {
                BufferUsages::empty()
            },
        mapped_at_creation: false,
    }
}
