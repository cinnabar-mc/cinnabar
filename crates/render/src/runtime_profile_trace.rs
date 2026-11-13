use render_scene::{RuntimeStage, RuntimeStageProfiler};

const SUBMISSION: usize = RuntimeStage::RenderSubmission as usize;
const GRAPH: usize = RuntimeStage::RenderGraph as usize;
const READBACK: usize = RuntimeStage::RenderReadback as usize;
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

/// Splits the root render graph into recording, readback, encoder finishing and submission.
/// The graph submits its own commands here so each step is timed; anything recorded later
/// still reaches the queue through the renderer's own submission before it presents.
fn install_graph_stages(world: &mut bevy::prelude::World) {
    use bevy::prelude::IntoScheduleConfigs;
    use bevy::render::renderer::{RenderGraph, RenderGraphSystems};
    use render_scene::{begin_stage_span, end_stage_span};
    let _ = world.try_schedule_scope(RenderGraph, |_, schedule| {
        schedule.add_systems((
            begin_stage_span::<GRAPH>.before(RenderGraphSystems::Begin),
            (end_stage_span::<GRAPH>, begin_stage_span::<READBACK>)
                .chain()
                .after(RenderGraphSystems::Render)
                .before(RenderGraphSystems::Submit),
            (
                end_stage_span::<READBACK>,
                submit_graph_commands,
                begin_stage_span::<PRESENT>,
            )
                .chain()
                .after(RenderGraphSystems::Finish),
        ));
    });
}

/// Finishes the graph's pending encoders, then submits them, as two timed stages. The queue
/// collects the finished buffers before submitting either way; `buffers` keeps its storage
/// between frames.
fn submit_graph_commands(
    mut pending: bevy::prelude::ResMut<bevy::render::renderer::PendingCommandBuffers>,
    queue: bevy::prelude::Res<bevy::render::renderer::RenderQueue>,
    profiler: Option<bevy::prelude::Res<RuntimeStageProfiler>>,
    mut buffers: bevy::prelude::Local<Vec<wgpu::CommandBuffer>>,
) {
    if pending.is_empty() {
        return;
    }
    let time = |stage| profiler.as_ref().map(|profiler| profiler.time(stage));
    let finish = time(RuntimeStage::EncodeFinish);
    buffers.extend(pending.finish());
    drop(finish);
    let _submit = time(RuntimeStage::QueueSubmit);
    queue.submit(buffers.drain(..));
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

    /// The graph records, reads back, finishes and submits as separate stages, leaving nothing
    /// pending, and its node and passes are attributed to the frame.
    #[test]
    fn graph_stages_split_recording_from_finishing_and_submission() {
        use crate::render_counting::CountedPasses as _;
        use bevy::prelude::*;
        use bevy::render::renderer::{
            PendingCommandBuffers, RenderContext, RenderGraph, RenderGraphSystems,
        };
        use render_scene::runtime_profile::render_counters::RenderCounter;
        let (device, queue) = crate::gpu_timing::tests::noop_device(wgpu::Features::empty());
        let profiler = RuntimeStageProfiler::new(true);
        let mut world = World::new();
        world.insert_resource(device);
        world.insert_resource(queue);
        world.insert_resource(profiler.clone());
        world.init_resource::<RuntimeStageSpans>();
        world.init_resource::<PendingCommandBuffers>();
        let mut graph = RenderGraph::base_schedule();
        graph.add_systems(
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
        );
        world.add_schedule(graph);
        super::install_graph_stages(&mut world);
        world.run_schedule(RenderGraph);
        assert!(world.resource::<PendingCommandBuffers>().is_empty());
        assert!(
            world
                .resource::<RuntimeStageSpans>()
                .is_open(RuntimeStage::SurfacePresent),
            "presentation is timed from the graph's submission"
        );
        let counts = profiler.finish_render_counters();
        assert!(counts.get(RenderCounter::Passes) >= 1);
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        for stage in [
            RuntimeStage::RenderGraph,
            RuntimeStage::NodeOpaque,
            RuntimeStage::RenderReadback,
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
