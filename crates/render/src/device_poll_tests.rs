use super::{FramesInFlight, MAX_FRAMES_IN_FLIGHT, PollTarget, poll_target};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Dependent steps of one GPU invocation per frame: far longer on any GPU than recording and
/// submitting the frame, so frames nothing paces pile up on the queue.
const STEPS_PER_FRAME: u32 = 1 << 22;

const SLOW_FRAME: &str = "
@group(0) @binding(0) var<storage, read_write> state: array<u32, 2>;

@compute @workgroup_size(1)
fn main() {
    var value = state[0];
    for (var step = 0u; step < state[1]; step++) {
        value = value * 1664525u + 1013904223u;
    }
    state[0] = value;
}
";

/// Requests a headless adapter: only real GPU execution can fall behind the CPU.
fn native_device() -> Option<(RenderDevice, RenderQueue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: headless GPU adapter for frames-in-flight pacing");
        return None;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&Default::default()))
        .expect("frames-in-flight device");
    Some((RenderDevice::from(device), RenderQueue::new(queue)))
}

/// Returns a submitter of one frame of dependent GPU work.
fn slow_frame(gpu: &wgpu::Device, queue: &wgpu::Queue) -> impl Fn(&wgpu::Queue) {
    let module = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("slow frame"),
        source: wgpu::ShaderSource::Wgsl(SLOW_FRAME.into()),
    });
    let pipeline = gpu.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("slow frame"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let state = gpu.create_buffer(&wgpu::BufferDescriptor {
        label: Some("slow frame state"),
        size: 8,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&state, 0, bytemuck::cast_slice(&[1u32, STEPS_PER_FRAME]));
    let bind_group = gpu.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("slow frame state"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: state.as_entire_binding(),
        }],
    });
    let gpu = gpu.clone();
    move |queue: &wgpu::Queue| {
        let mut encoder = gpu.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit([encoder.finish()]);
    }
}

/// Hidden clients render with no swap chain to wait on. When the GPU is slower than the CPU,
/// each frame's poll must still keep the queued frames, and the memory they pin, bounded.
#[test]
fn frames_without_a_swap_chain_stay_within_the_in_flight_bound() {
    let Some((device, queue)) = native_device() else {
        return;
    };
    let gpu = device.wgpu_device();
    let slow = slow_frame(gpu, &queue);
    let completed = Arc::new(AtomicUsize::new(0));
    let mut frames = FramesInFlight::default();
    for submitted in 1..=4 * MAX_FRAMES_IN_FLIGHT {
        slow(&queue);
        let receipt = Arc::clone(&completed);
        queue.on_submitted_work_done(move || {
            receipt.fetch_add(1, Ordering::AcqRel);
        });
        frames.finish_frame(gpu, &queue).expect("frame poll");
        let queued = submitted - completed.load(Ordering::Acquire);
        assert!(
            queued <= MAX_FRAMES_IN_FLIGHT,
            "{queued} frames still queued after frame {submitted}"
        );
    }
}

/// A candidate's slow frame completes in its own poll, even after ordinary frames filled the
/// in-flight queue, and the post-poll surface copy stays tracked.
#[test]
#[cfg(all(feature = "frame-generation", not(target_arch = "wasm32")))]
fn frame_generation_wait_tracks_each_completed_candidate_and_post_poll_copy() {
    let Some((device, queue)) = native_device() else {
        return;
    };
    let mut frames = FramesInFlight::default();
    // Ordinary frames leave the in-flight queue full before the first candidate.
    for _ in 0..=MAX_FRAMES_IN_FLIGHT {
        queue.submit([]);
        frames.finish_frame(device.wgpu_device(), &queue).unwrap();
    }
    let slow = slow_frame(device.wgpu_device(), &queue);
    let completed = Arc::new(AtomicUsize::new(0));
    for pair in 1..=8 {
        slow(&queue);
        let receipt = Arc::clone(&completed);
        queue.on_submitted_work_done(move || {
            receipt.fetch_add(1, Ordering::AcqRel);
        });
        frames
            .finish_with_wait(
                device.wgpu_device(),
                &queue,
                Some(std::time::Duration::from_secs(1)),
            )
            .unwrap();
        assert_eq!(completed.load(Ordering::Acquire), pair);
        queue.submit([]);
        frames.track_tail(&queue);
    }
}

/// A candidate waits for its own frame even when ordinary frames filled the in-flight queue.
#[test]
fn a_candidate_waits_for_the_newest_frame_in_a_full_queue() {
    let deadline = std::time::Duration::from_millis(16);
    for outstanding in [1, MAX_FRAMES_IN_FLIGHT, MAX_FRAMES_IN_FLIGHT + 1] {
        assert_eq!(
            poll_target(outstanding, Some(deadline)),
            PollTarget::Newest(deadline)
        );
    }
    assert_eq!(
        poll_target(MAX_FRAMES_IN_FLIGHT + 1, None),
        PollTarget::Oldest
    );
    assert_eq!(poll_target(MAX_FRAMES_IN_FLIGHT, None), PollTarget::None);
}
