use super::{FramesInFlight, MAX_FRAMES_IN_FLIGHT};
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

/// Hidden clients render with no swap chain to wait on. When the GPU is slower than the CPU,
/// each frame's poll must still keep the queued frames, and the memory they pin, bounded.
#[test]
fn frames_without_a_swap_chain_stay_within_the_in_flight_bound() {
    let Some((device, queue)) = native_device() else {
        return;
    };
    let gpu = device.wgpu_device();
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
    let completed = Arc::new(AtomicUsize::new(0));
    let mut frames = FramesInFlight::default();
    for submitted in 1..=4 * MAX_FRAMES_IN_FLIGHT {
        let mut encoder = gpu.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit([encoder.finish()]);
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
