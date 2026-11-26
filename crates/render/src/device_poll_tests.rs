use super::{FrameSubmissions, FramesInFlight, MAX_FRAMES_IN_FLIGHT, submission_marker};
use bevy::{
    prelude::{IntoScheduleConfigs, Res, ResMut, Resource},
    render::{
        Render, RenderSystems,
        renderer::{PendingCommandBuffers, RenderDevice, RenderQueue},
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
    let frames = FramesInFlight::default();
    for submitted in 1..=4 * MAX_FRAMES_IN_FLIGHT {
        let mut encoder = gpu.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        let submission = queue.submit([encoder.finish()]);
        let receipt = Arc::clone(&completed);
        queue.on_submitted_work_done(move || {
            receipt.fetch_add(1, Ordering::AcqRel);
        });
        frames.finish_frame(gpu, submission).expect("frame poll");
        let queued = submitted - completed.load(Ordering::Acquire);
        assert!(
            queued <= MAX_FRAMES_IN_FLIGHT,
            "{queued} frames still queued after frame {submitted}"
        );
    }
}

/// Stands in for the render graph: records one command and submits the frame.
fn submit_frame(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    pending
        .command_encoder(&device)
        .insert_debug_marker("frame");
    pending.submit(&queue);
}

/// The poll bounds a frame by the frame's own submission. An empty submission to obtain an index
/// would cost a driver submission on every frame.
#[test]
fn polling_a_frame_makes_no_submission_of_its_own() {
    let (mut app, _) = crate::queue_review_support::app();
    app.add_schedule(Render::base_schedule())
        .init_resource::<PendingCommandBuffers>();
    super::install(app.main_mut());
    app.add_systems(
        Render,
        submit_frame
            .in_set(RenderSystems::Render)
            .before(FrameSubmissions),
    );
    let queue = app.world().resource::<RenderQueue>().clone();
    for frame in 0..2 * MAX_FRAMES_IN_FLIGHT {
        let before = submission_marker(&queue);
        app.world_mut().run_schedule(Render);
        // The frame's submission and the marker read itself.
        assert_eq!(submission_marker(&queue) - before, 2, "frame {frame}");
        assert_eq!(
            app.world()
                .resource::<FramesInFlight>()
                .0
                .lock()
                .unwrap()
                .len(),
            (frame + 1).min(MAX_FRAMES_IN_FLIGHT),
            "frame {frame} is bounded by its own submission"
        );
    }
}

/// A buffer the frame writes through the queue's staging, without recording any commands.
#[derive(Resource)]
struct StagedBuffer(wgpu::Buffer);

const STAGED: [u8; 4] = [7, 1, 9, 3];

fn stage_write(queue: Res<RenderQueue>, staged: Res<StagedBuffer>) {
    queue.write_buffer(&staged.0, 0, &STAGED);
}

/// A render app on the NOOP device whose frames stage one write and submit no graph work.
fn writes_only_app() -> (bevy::app::App, wgpu::Buffer) {
    let (mut app, _) = crate::queue_review_support::app();
    app.add_schedule(Render::base_schedule())
        .init_resource::<PendingCommandBuffers>();
    super::install(app.main_mut());
    let buffer = app
        .world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .create_buffer(&wgpu::BufferDescriptor {
            label: Some("staged frame write"),
            size: STAGED.len() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    app.insert_resource(StagedBuffer(buffer.clone()));
    app.add_systems(
        Render,
        stage_write
            .in_set(RenderSystems::Render)
            .before(FrameSubmissions),
    );
    (app, buffer)
}

/// A frame whose graph submits nothing, such as one with surface acquisition suspended, still
/// sends the writes it staged and still counts towards the in-flight bound.
#[test]
fn a_frame_without_a_graph_submission_sends_its_staged_writes() {
    let (mut app, buffer) = writes_only_app();
    let queue = app.world().resource::<RenderQueue>().clone();
    let before = submission_marker(&queue);
    app.world_mut().run_schedule(Render);
    // The poll's own submission and the marker read itself.
    assert_eq!(submission_marker(&queue) - before, 2);
    assert_eq!(
        app.world()
            .resource::<FramesInFlight>()
            .0
            .lock()
            .unwrap()
            .len(),
        1
    );
    let mapped = Arc::new(AtomicBool::new(false));
    let receipt = Arc::clone(&mapped);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            result.expect("staged buffer maps");
            receipt.store(true, Ordering::Release);
        });
    app.world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    assert!(
        mapped.load(Ordering::Acquire),
        "the staged write was never submitted"
    );
    let written = buffer
        .slice(..)
        .get_mapped_range()
        .expect("mapped staged buffer");
    assert_eq!(*written, STAGED);
}

/// Stands in for an owner that submits its own work after the graph, like the Metal queue probe.
fn submit_after_graph(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let buffer = device.create_command_encoder(&Default::default()).finish();
    pending.push([buffer]);
    pending.submit(&queue);
}

/// Work submitted after the graph through the pending command buffers is the frame's bound.
#[test]
fn work_submitted_after_the_graph_is_inside_the_frame_bound() {
    let (mut app, _) = crate::queue_review_support::app();
    app.add_schedule(Render::base_schedule())
        .init_resource::<PendingCommandBuffers>();
    super::install(app.main_mut());
    app.add_systems(
        Render,
        (
            submit_frame
                .in_set(RenderSystems::Render)
                .before(FrameSubmissions),
            submit_after_graph.in_set(FrameSubmissions),
        ),
    );
    let queue = app.world().resource::<RenderQueue>().clone();
    let before = submission_marker(&queue);
    app.world_mut().run_schedule(Render);
    let after = submission_marker(&queue);
    // The graph's submission, the late one, and the marker read itself: no poll submission.
    assert_eq!(after - before, 3);
    let bound = format!(
        "{:?}",
        app.world()
            .resource::<FramesInFlight>()
            .0
            .lock()
            .unwrap()
            .back()
    );
    let bound: u64 = bound
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("recorded index");
    assert_eq!(bound + 1, after, "the late submission bounds the frame");
}

/// With a render thread that runs the frame tail, the poll moves behind the hand-back of the
/// render app: the frame takes its submission index, and the tail records it and polls.
#[test]
fn a_render_thread_tail_polls_after_the_frame() {
    use bevy::render::renderer::{FrameTailOnRenderThread, RenderFrameTail};
    let (mut app, _) = crate::queue_review_support::app();
    app.add_schedule(Render::base_schedule())
        .init_resource::<PendingCommandBuffers>()
        .init_resource::<RenderFrameTail>()
        .insert_resource(FrameTailOnRenderThread);
    super::install(app.main_mut());
    app.add_systems(
        Render,
        submit_frame
            .in_set(RenderSystems::Render)
            .before(FrameSubmissions),
    );
    let polls = app.world().resource::<super::DevicePolls>().clone();
    let frames = app.world().resource::<FramesInFlight>().clone();
    let tail = app.world().resource::<RenderFrameTail>().clone();

    app.world_mut().run_schedule(Render);
    assert_eq!(polls.count(), 0, "the frame leaves the poll to the tail");
    assert!(frames.0.lock().unwrap().is_empty());
    tail.run();
    assert_eq!(polls.count(), 1);
    assert_eq!(
        frames.0.lock().unwrap().len(),
        1,
        "the tail bounds the frame's submission"
    );
}
