//! Uploads the first-person rig's instance and bone streams.
use super::*;

/// The pose allocations a hand rig frame draws from.
pub(super) struct PoseStreams {
    instances: std::sync::Arc<[render_model::ActorGpuInstance]>,
    previous_bones: std::sync::Arc<[[[f32; 4]; 3]]>,
    current_bones: std::sync::Arc<[[[f32; 4]; 3]]>,
}

/// Uploads the current rig pose and instance stream into retained buffers; false when out of
/// memory kept the previous pose.
#[must_use]
pub(in crate::hand_rig_render) fn upload_pose(
    gpu: &mut HandRigGpu,
    uploads: &crate::upload_staging::FrameUploads<'_>,
    frame: &HandRigFrame,
) -> bool {
    if gpu.publication == Some(frame.publication) && gpu.instances.is_some() {
        return true;
    }
    let compact = !gpu.layout.entries.iter().any(|entry| entry.binding == 5);
    if compact {
        if frame.rig.previous_bones.len() != frame.rig.current_bones.len() {
            return false;
        }
        gpu.compact_pose.clear();
        gpu.compact_pose
            .extend_from_slice(bytemuck::cast_slice(&frame.rig.previous_bones));
        gpu.compact_pose
            .extend_from_slice(bytemuck::cast_slice(&frame.rig.current_bones));
    }
    let stream_count = if compact { 2 } else { 3 };
    // A stream whose allocation the buffers already hold is unchanged: scene-only publications,
    // such as new artwork, keep the pose allocations.
    let same = |held: &PoseStreams| {
        let instances = std::sync::Arc::ptr_eq(&held.instances, &frame.rig.instances);
        let previous = std::sync::Arc::ptr_eq(&held.previous_bones, &frame.rig.previous_bones);
        let current = std::sync::Arc::ptr_eq(&held.current_bones, &frame.rig.current_bones);
        [
            instances,
            if compact {
                previous && current
            } else {
                previous
            },
            current,
        ]
    };
    let unchanged = gpu.uploaded_pose.as_ref().map_or([false; 3], same);
    // The pose changes every frame; same-sized buffers are rewritten so the bind group survives.
    let streams = [
        (
            "first-person rig instance",
            bytemuck::cast_slice::<_, u8>(&frame.rig.instances),
        ),
        (
            "first-person rig previous bones",
            if compact {
                &gpu.compact_pose
            } else {
                bytemuck::cast_slice::<_, u8>(&frame.rig.previous_bones)
            },
        ),
        (
            "first-person rig current bones",
            bytemuck::cast_slice::<_, u8>(&frame.rig.current_bones),
        ),
    ];
    let slots = [&gpu.instances, &gpu.previous_bones, &gpu.current_bones];
    // Resized streams are all allocated before any write, so out of memory keeps the last
    // consistent pose and retries next frame.
    let mut replacements: [Option<Buffer>; 3] = [None, None, None];
    for ((slot, (label, bytes)), replacement) in slots
        .iter()
        .zip(streams)
        .zip(&mut replacements)
        .take(stream_count)
    {
        if slot
            .as_ref()
            .is_some_and(|buffer| buffer.size() == bytes.len() as u64)
        {
            continue;
        }
        let Some(buffer) = crate::gpu_allocation::buffer_with_data(
            &uploads.device,
            &uploads.queue,
            label,
            bytes,
            BufferUsages::STORAGE,
        ) else {
            return false;
        };
        *replacement = Some(buffer);
    }
    let recreated = replacements.iter().any(Option::is_some);
    let slots = [
        &mut gpu.instances,
        &mut gpu.previous_bones,
        &mut gpu.current_bones,
    ];
    #[cfg(test)]
    let mut writes = 0;
    for (((slot, (_label, bytes)), replacement), unchanged) in slots
        .into_iter()
        .zip(streams)
        .zip(replacements)
        .zip(unchanged)
        .take(stream_count)
    {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "hand.pose_upload",
            label = _label,
            revision = frame.revision,
            bytes = bytes.len()
        )
        .entered();
        match (replacement, slot.as_ref()) {
            (Some(buffer), _) => *slot = Some(buffer),
            (None, Some(_)) if unchanged => {}
            (None, Some(buffer)) => {
                uploads.write(buffer, 0, bytes);
                #[cfg(test)]
                {
                    writes += 1;
                }
            }
            (None, None) => unreachable!("a missing stream is always replaced"),
        }
    }
    #[cfg(test)]
    {
        gpu.pose_writes += writes;
    }
    gpu.uploaded_pose = Some(PoseStreams {
        instances: std::sync::Arc::clone(&frame.rig.instances),
        previous_bones: std::sync::Arc::clone(&frame.rig.previous_bones),
        current_bones: std::sync::Arc::clone(&frame.rig.current_bones),
    });
    gpu.maximum_vertex_count = frame.rig.maximum_vertex_count;
    gpu.instance_count = u32::try_from(frame.rig.instances.len()).unwrap_or(0);
    gpu.publication = Some(frame.publication);
    if recreated {
        gpu.bind_group = None;
    }
    true
}
