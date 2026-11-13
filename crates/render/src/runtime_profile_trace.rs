use render_scene::{RuntimeStage, RuntimeStageProfiler};

const SUBMISSION: usize = RuntimeStage::RenderSubmission as usize;
const GRAPH: usize = RuntimeStage::RenderGraph as usize;
const SUBMIT: usize = RuntimeStage::QueueSubmit as usize;
const PRESENT: usize = RuntimeStage::SurfacePresent as usize;

/// Brackets surface preparation to distinguish drawable waits from game work, and splits
/// submission into its stages.
pub(crate) fn install_surface_trace(app: &mut bevy::app::SubApp) {
    use bevy::prelude::*;
    use bevy::render::{
        Render, RenderStartup, RenderSystems, renderer::render_system,
        view::window::prepare_windows,
    };
    const SURFACE: usize = RuntimeStage::SurfacePreparation as usize;
    const FRAME: usize = RuntimeStage::RenderFrame as usize;
    app.add_systems(bevy::render::ExtractSchedule, extract_frame_identity)
        .init_resource::<render_scene::RuntimeStageSpans>()
        .add_systems(RenderStartup, install_graph_stages)
        .add_systems(
            Render,
            (
                render_scene::begin_stage_span::<SURFACE>.before(prepare_windows),
                render_scene::end_stage_span::<SURFACE>.after(prepare_windows),
            )
                .in_set(RenderSystems::PrepareViews),
        )
        // Spans all render-world work; the acquisition wait is subtracted at the end.
        .add_systems(
            Render,
            (
                render_scene::begin_stage_span::<FRAME>.before(RenderSystems::ExtractCommands),
                (
                    render_scene::runtime_profile::end_render_frame_span,
                    finish_render_counters,
                )
                    .in_set(RenderSystems::Cleanup),
            ),
        )
        .add_systems(
            Render,
            (
                render_scene::begin_stage_span::<SUBMISSION>.before(render_system),
                (
                    render_scene::end_stage_span::<PRESENT>,
                    render_scene::end_stage_span::<SUBMISSION>,
                )
                    .chain()
                    .after(render_system)
                    // The device poll after these owners may wait for the GPU.
                    .before(crate::device_poll::FrameSubmissions),
            )
                .in_set(RenderSystems::Render),
        );
}

/// Splits the root render graph into recording, encoder finishing and submission. Nothing here
/// submits: the renderer's own system in the graph's Submit set still makes the frame's one
/// submission.
fn install_graph_stages(world: &mut bevy::prelude::World) {
    use bevy::prelude::IntoScheduleConfigs;
    use bevy::render::renderer::{
        RenderGraph, RenderGraphSystems, encode_screenshot_and_readback_commands,
    };
    use render_scene::{begin_stage_span, end_stage_span};
    let _ = world.try_schedule_scope(RenderGraph, |_, schedule| {
        schedule.add_systems((
            begin_stage_span::<GRAPH>.before(RenderGraphSystems::Begin),
            // The renderer submits after its screenshot and readback copies; everything the
            // nodes recorded is pending by the time the Submit set starts.
            (
                end_stage_span::<GRAPH>,
                finish_graph_encoders,
                begin_stage_span::<SUBMIT>,
            )
                .chain()
                .after(RenderGraphSystems::Render)
                .in_set(RenderGraphSystems::Submit)
                .before(encode_screenshot_and_readback_commands),
            end_stage_span::<SUBMIT>
                .after(RenderGraphSystems::Submit)
                .before(RenderGraphSystems::Finish),
            begin_stage_span::<PRESENT>
                .after(RenderGraphSystems::Submit)
                .after(RenderGraphSystems::Finish),
        ));
    });
}

/// Finishes the encoders the graph's nodes recorded and queues the finished buffers again in
/// the same order, so encoding is timed apart from the submission that follows. The renderer
/// would finish the same encoders in the same order while submitting; commands recorded after
/// this, such as screenshot copies, go into a command buffer of their own behind them.
/// `buffers` keeps its storage between frames.
fn finish_graph_encoders(
    mut pending: bevy::prelude::ResMut<bevy::render::renderer::PendingCommandBuffers>,
    profiler: Option<bevy::prelude::Res<RuntimeStageProfiler>>,
    mut buffers: bevy::prelude::Local<Vec<wgpu::CommandBuffer>>,
) {
    let Some(profiler) = profiler else {
        return;
    };
    if pending.is_empty() {
        return;
    }
    let _finish = profiler.time(RuntimeStage::EncodeFinish);
    buffers.extend(pending.finish());
    pending.push(buffers.drain(..));
}

/// Closes the frame's work counters once every render system has run.
fn finish_render_counters(profiler: Option<bevy::prelude::Res<RuntimeStageProfiler>>) {
    if let Some(profiler) = profiler {
        profiler.finish_render_counters();
    }
}

/// Carries the input identity into the render world before its queued commands run.
fn extract_frame_identity(
    mut commands: bevy::prelude::Commands,
    profiler: bevy::render::Extract<bevy::prelude::Res<render_scene::RuntimeStageProfiler>>,
) {
    commands.insert_resource(profiler.for_frame());
}

#[cfg(test)]
mod tests {
    use render_scene::{RuntimeStage, RuntimeStageProfiler, RuntimeStageSpans};
    use std::time::Duration;

    /// The render-frame span is open from asset preparation through rendering, closed by Cleanup.
    #[test]
    fn render_frame_span_covers_asset_preparation_through_cleanup() {
        use bevy::prelude::*;
        use bevy::render::{Render, RenderSystems};
        const FRAME: usize = RuntimeStage::RenderFrame as usize;
        #[derive(Resource, Default)]
        struct Open(Vec<bool>);
        /// Records whether the frame span covers the current render stage.
        fn probe(spans: Res<RuntimeStageSpans>, mut open: ResMut<Open>) {
            open.0.push(spans.is_open(RuntimeStage::RenderFrame));
        }
        let profiler = RuntimeStageProfiler::new(true);
        let mut app = App::new();
        app.add_schedule(Render::base_schedule())
            .insert_resource(profiler.clone())
            .init_resource::<Open>();
        crate::runtime_profile_trace::install_surface_trace(app.main_mut());
        app.add_systems(
            Render,
            (
                probe.in_set(RenderSystems::PrepareAssets),
                probe.in_set(RenderSystems::Render),
                probe.in_set(RenderSystems::PostCleanup),
            )
                .chain(),
        );
        app.world_mut().run_schedule(Render);
        assert_eq!(app.world().resource::<Open>().0, [true, true, false]);
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        assert_eq!(snapshot.samples[FRAME].count, 1);
    }

    /// Owners that run after the frame's submissions, and the device poll behind them, are
    /// outside the submission span.
    #[test]
    fn submission_span_closes_before_frame_submission_owners() {
        use bevy::prelude::*;
        use bevy::render::{Render, RenderSystems};
        #[derive(Resource, Default)]
        struct OpenAtOwners(Option<bool>);
        /// Records whether submission is still being timed when a post-submission owner runs.
        fn probe(spans: Res<RuntimeStageSpans>, mut open: ResMut<OpenAtOwners>) {
            open.0 = Some(spans.is_open(RuntimeStage::RenderSubmission));
        }
        let profiler = RuntimeStageProfiler::new(true);
        let mut app = App::new();
        app.add_schedule(Render::base_schedule())
            .insert_resource(profiler.clone())
            .init_resource::<OpenAtOwners>();
        crate::runtime_profile_trace::install_surface_trace(app.main_mut());
        app.configure_sets(
            Render,
            crate::device_poll::FrameSubmissions.in_set(RenderSystems::Render),
        )
        .add_systems(Render, probe.in_set(crate::device_poll::FrameSubmissions));
        app.world_mut().run_schedule(Render);
        assert_eq!(app.world().resource::<OpenAtOwners>().0, Some(false));
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        assert_eq!(
            snapshot.samples[RuntimeStage::RenderSubmission as usize].count,
            1
        );
    }

    /// The graph's recording, encoder finishing and submission are separate stages, and the
    /// finished command buffers stay queued for the renderer's own submission.
    #[test]
    fn graph_stages_split_recording_from_finishing_and_submission() {
        use crate::render_counting::CountedPasses as _;
        use bevy::prelude::*;
        use bevy::render::renderer::{
            PendingCommandBuffers, RenderContext, RenderGraph, RenderGraphSystems, RenderQueue,
            encode_screenshot_and_readback_commands,
        };
        use render_scene::runtime_profile::render_counters::RenderCounter;
        #[derive(Resource, Default)]
        struct Submitted(usize);
        /// Stands in for the renderer's submission, where the core pipeline runs it.
        fn submit(
            mut pending: ResMut<PendingCommandBuffers>,
            queue: Res<RenderQueue>,
            mut submitted: ResMut<Submitted>,
        ) {
            submitted.0 += pending.len();
            queue.submit(pending.finish());
        }
        let (device, queue) = crate::gpu_timing::tests::noop_device(wgpu::Features::empty());
        let profiler = RuntimeStageProfiler::new(true);
        let mut world = World::new();
        world.insert_resource(device);
        world.insert_resource(queue);
        world.insert_resource(profiler.clone());
        world.init_resource::<RuntimeStageSpans>();
        world.init_resource::<PendingCommandBuffers>();
        world.init_resource::<Submitted>();
        let mut graph = RenderGraph::base_schedule();
        graph.add_systems((
            crate::gpu_timing::profiled(
                |mut context: RenderContext| {
                    drop(
                        context
                            .command_encoder()
                            .counted_compute_pass(&Default::default()),
                    );
                },
                None,
                RuntimeStage::NodeOpaque,
                "fixture node",
            )
            .in_set(RenderGraphSystems::Render),
            submit
                .in_set(RenderGraphSystems::Submit)
                .after(encode_screenshot_and_readback_commands),
        ));
        world.add_schedule(graph);
        super::install_graph_stages(&mut world);
        world.run_schedule(RenderGraph);
        assert_eq!(
            world.resource::<Submitted>().0,
            1,
            "the node's commands reach the renderer's submission as one buffer"
        );
        assert!(world.resource::<PendingCommandBuffers>().is_empty());
        assert!(
            world
                .resource::<RuntimeStageSpans>()
                .is_open(RuntimeStage::SurfacePresent),
            "presentation is timed from the end of the graph"
        );
        let counts = profiler.finish_render_counters();
        assert!(counts.get(RenderCounter::Passes) >= 1);
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        for stage in [
            RuntimeStage::RenderGraph,
            RuntimeStage::NodeOpaque,
            RuntimeStage::EncodeFinish,
            RuntimeStage::QueueSubmit,
        ] {
            assert_eq!(
                snapshot.samples[stage as usize].count,
                1,
                "{}",
                stage.name()
            );
        }
    }
}
