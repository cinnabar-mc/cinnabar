//! Retained model palettes share the UI pipeline and its existing ordered draw calls.

use super::*;
use render_model::{UiModelBone, UiModelPose};

pub(super) struct UiModels {
    pub(super) buffer: Buffer,
    capacity: usize,
    poses: Arc<[UiModelPose]>,
    bones: Vec<UiModelBone>,
    scratch: Vec<UiModelBone>,
    resident: bool,
}

impl UiModels {
    /// Includes the shared palette arena in retained GPU memory accounting.
    pub(super) fn bytes(&self) -> usize {
        self.capacity * size_of::<UiModelBone>()
    }
    /// Supplies a valid storage binding even when no model is visible.
    pub(super) fn new(device: &RenderDevice) -> Self {
        Self {
            buffer: device.create_buffer(&BufferDescriptor {
                label: Some("retained UI model bone palette"),
                size: size_of::<UiModelBone>() as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            capacity: 1,
            poses: Arc::from([]),
            bones: Vec::new(),
            scratch: Vec::new(),
            resident: false,
        }
    }

    /// Uploads changed bones, growing the shared arena only when its capacity is exhausted.
    /// Returns whether texture buckets need the replacement storage binding.
    pub(super) fn prepare(
        &mut self,
        poses: &Arc<[UiModelPose]>,
        uploads: &crate::upload_staging::FrameUploads<'_>,
        profile: Option<&super::profile::UiProfile>,
    ) -> Result<bool, UiRenderRejectReason> {
        if self.resident && Arc::ptr_eq(&self.poses, poses) {
            return Ok(false);
        }
        self.scratch.clear();
        for pose in poses.iter() {
            self.scratch.extend_from_slice(&pose.bones);
        }
        let fresh = self.scratch.len() > self.capacity;
        let replacement = if fresh {
            let capacity = self.scratch.len().next_power_of_two();
            let bytes = (capacity * size_of::<UiModelBone>()) as u64;
            let buffer = crate::gpu_allocation::buffer(
                &uploads.device,
                &BufferDescriptor {
                    label: Some("retained UI model bone palette"),
                    size: bytes,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                },
            )
            .ok_or(UiRenderRejectReason::GeometryAllocationFailed { bytes })?;
            Some((buffer, capacity))
        } else {
            None
        };
        let target = replacement
            .as_ref()
            .map_or(&self.buffer, |(buffer, _)| buffer);
        let changed =
            super::uploads::changed_range(&self.bones, &self.scratch, fresh || !self.resident);
        if !changed.is_empty() {
            let bytes = bytemuck::cast_slice(&self.scratch[changed.clone()]);
            let size = bytes.len() as u64;
            let written = crate::gpu_allocation::checked(
                &uploads.device,
                "UI model palette upload",
                size,
                || {
                    uploads.write(
                        target,
                        (changed.start * size_of::<UiModelBone>()) as u64,
                        bytes,
                    );
                },
            );
            if written.is_none() {
                // A failed in-place write may have changed a prefix of the resident palette.
                self.resident &= fresh;
                return Err(UiRenderRejectReason::GeometryAllocationFailed { bytes: size });
            }
            if let Some(profile) = profile {
                profile.record_upload(super::profile::UploadKind::Geometry, bytes.len() as u64);
            }
        }
        if let Some((buffer, capacity)) = replacement {
            self.buffer = buffer;
            self.capacity = capacity;
        }
        std::mem::swap(&mut self.bones, &mut self.scratch);
        self.poses = Arc::clone(poses);
        self.resident = true;
        Ok(fresh)
    }

    /// Packs the palette base above the texture-array layer in the draw's instance index.
    pub(super) fn instance(&self, batch: &UiRenderBatch, layer: u32) -> u32 {
        render_model::ui_model_instance(&self.poses, batch.isolated_depth_scope, layer)
    }
}
