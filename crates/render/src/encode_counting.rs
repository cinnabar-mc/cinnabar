//! Feeds the GPU API's own command-encoding counters to the frame profiler, so a slow frame
//! says where finishing its encoders went without a debugger attached.

use bevy::{prelude::Res, render::renderer::RenderDevice};
use render_scene::{
    RuntimeStageProfiler,
    runtime_profile_encode::{EncodeCounter, EncodeCounts, publish},
};

/// Reads `counter` from the device's totals: counts as counted, durations in whole
/// microseconds. Every counter has a source here, or this does not compile.
fn total(counter: EncodeCounter, counters: &wgpu::InternalCounters) -> u64 {
    use EncodeCounter as C;
    let (core, hal) = (&counters.core.encode, &counters.hal.encode);
    let (value, nanoseconds) = match counter {
        C::Encoders => (&core.encoders, false),
        C::FinishMicros => (&core.finish_ns, true),
        C::PassPrepareMicros => (&core.pass_prepare_ns, true),
        C::PassReplayMicros => (&core.pass_replay_ns, true),
        C::RenderPasses => (&hal.render_passes, false),
        C::RenderPassBeginMicros => (&hal.render_pass_begin_ns, true),
        C::FramebuffersCreated => (&hal.framebuffers_created, false),
        C::FramebufferCreateMicros => (&hal.framebuffer_create_ns, true),
        C::TextureCopies => (&hal.texture_copies, false),
        C::OtherCopies => (&hal.other_copies, false),
        C::CopyMicros => (&hal.copy_ns, true),
        C::Barriers => (&hal.barriers, false),
        C::BarrierMicros => (&hal.barrier_ns, true),
        C::CommandBufferEndMicros => (&hal.command_buffer_end_ns, true),
        C::PoolResetMicros => (&hal.command_pool_reset_ns, true),
    };
    let value = u64::try_from(value.read()).unwrap_or(0);
    if nanoseconds { value / 1_000 } else { value }
}

/// The device's encoding totals since it was created, in report units.
pub(crate) fn totals(device: &wgpu::Device) -> EncodeCounts {
    let counters = device.get_internal_counters();
    EncodeCounts(EncodeCounter::ALL.map(|counter| total(counter, &counters)))
}

/// Publishes the device's encoding totals once the render frame has encoded and submitted.
/// Costs one read of each of the device's counters per frame, and nothing without a profiler.
pub(crate) fn publish_encode_counters(
    device: Option<Res<RenderDevice>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    if let (Some(device), Some(_)) = (device, profiler) {
        publish(totals(device.wgpu_device()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finishing an encoder moves the device's totals.
    #[test]
    fn finishing_an_encoder_adds_to_the_device_totals() {
        let (device, queue) = crate::gpu_timing::tests::noop_device(wgpu::Features::empty());
        let device = device.wgpu_device();
        let before = totals(device);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 64,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        drop(encoder.begin_compute_pass(&Default::default()));
        encoder.clear_buffer(&buffer, 0, None);
        queue.submit([encoder.finish()]);
        let added = totals(device).since(&before);
        assert_eq!(added.get(EncodeCounter::Encoders), 1);
        assert_eq!(
            added.get(EncodeCounter::RenderPasses),
            0,
            "the no-op backend counts no backend work"
        );
    }
}
