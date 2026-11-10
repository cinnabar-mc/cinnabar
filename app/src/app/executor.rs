//! Executor choice for the schedules of the main and render worlds.

use bevy::{
    app::{App, SubApp},
    ecs::schedule::{
        MainThreadExecutor, MultiThreadedExecutor, Schedules, SingleThreadedExecutor,
        SystemExecutor,
    },
    render::{Render, RenderApp, renderer::SerialRenderExecutor},
};

/// Minimizes schedule task wake-ups while preserving Apple surface work's UI-thread dispatch.
/// Call after plugins have added their schedules and before pipelined-rendering cleanup.
pub(super) fn configure_frame_schedule_executors(app: &mut App) {
    let needs_ui_dispatch = cfg!(any(target_os = "macos", target_os = "ios"))
        && app.world().contains_resource::<MainThreadExecutor>();
    run_schedules_on_one_thread(app.main_mut(), SingleThreadedExecutor::new);
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        // The serial render executor is single-threaded too, and also lets render systems record
        // into the frame's one command encoder instead of finishing an encoder each.
        run_schedules_on_one_thread(render_app, SerialRenderExecutor::new);
        // Pipelined rendering moves Render to its own thread. Only the parallel
        // executor sends NonSend surface systems back through MainThreadExecutor.
        if needs_ui_dispatch && let Some(schedule) = render_app.get_schedule_mut(Render) {
            schedule.set_executor(MultiThreadedExecutor::new());
        }
    }
}

/// Sets schedules to execute directly on their calling thread with `executor`.
fn run_schedules_on_one_thread<E: SystemExecutor + 'static>(
    app: &mut SubApp,
    executor: impl Fn() -> E,
) {
    let Some(mut schedules) = app.world_mut().get_resource_mut::<Schedules>() else {
        return;
    };
    for (_, schedule) in schedules.iter_mut() {
        schedule.set_executor(executor());
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        app::Update,
        ecs::{
            schedule::{Schedule, ScheduleLabel},
            system::NonSendMarker,
        },
        prelude::{IntoScheduleConfigs, ResMut, Resource},
    };
    use std::thread::{self, ThreadId};

    use super::*;

    #[derive(Resource, Default)]
    struct ExecutionThreads(Vec<ThreadId>);

    #[derive(Resource, Default)]
    struct SubmissionThreads(Vec<ThreadId>);

    /// Records which thread performed a system's thread-affine work.
    fn record_thread(_marker: NonSendMarker, mut threads: ResMut<ExecutionThreads>) {
        threads.0.push(thread::current().id());
    }

    /// Records render submission that belongs to the schedule's calling thread.
    fn record_submission(world: &mut bevy::prelude::World) {
        world
            .resource_mut::<SubmissionThreads>()
            .0
            .push(thread::current().id());
    }

    #[test]
    fn main_world_thread_affine_work_stays_on_its_owner() {
        let mut app = App::new();
        app.init_resource::<ExecutionThreads>()
            .add_systems(Update, record_thread);
        configure_frame_schedule_executors(&mut app);
        app.update();
        assert_eq!(
            app.world().resource::<ExecutionThreads>().0,
            [thread::current().id()]
        );
    }

    #[test]
    fn non_pipelined_render_work_stays_on_its_owner() {
        let mut app = App::new();
        let mut render_app = SubApp::new();
        render_app.update_schedule = Some(Render.intern());
        render_app
            .add_schedule(Schedule::new(Render))
            .init_resource::<ExecutionThreads>()
            .init_resource::<SubmissionThreads>()
            .add_systems(Render, (record_thread, record_submission));
        app.insert_sub_app(RenderApp, render_app);
        configure_frame_schedule_executors(&mut app);
        app.update();
        let world = app.sub_app(RenderApp).world();
        assert_eq!(
            world.resource::<ExecutionThreads>().0,
            [thread::current().id()]
        );
        assert_eq!(
            world.resource::<SubmissionThreads>().0,
            [thread::current().id()]
        );
    }

    /// Records an empty compute pass through the system's render context.
    fn record_pass(mut context: bevy::render::renderer::RenderContext) {
        context
            .command_encoder()
            .begin_compute_pass(&wgpu::ComputePassDescriptor::default());
    }

    #[test]
    fn render_systems_record_into_one_command_buffer() {
        use bevy::render::renderer::{PendingCommandBuffers, RenderDevice};
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default()))
        else {
            eprintln!("missing fixture: headless GPU adapter for render command recording");
            return;
        };
        let (device, _queue) = bevy::tasks::block_on(adapter.request_device(&Default::default()))
            .expect("render command recording device");
        let mut app = App::new();
        let mut render_app = SubApp::new();
        render_app.update_schedule = Some(Render.intern());
        render_app
            .add_schedule(Schedule::new(Render))
            .insert_resource(RenderDevice::from(device))
            .init_resource::<PendingCommandBuffers>()
            .add_systems(Render, (record_pass, record_pass).chain());
        app.insert_sub_app(RenderApp, render_app);
        configure_frame_schedule_executors(&mut app);
        app.update();
        let world = app.sub_app(RenderApp).world();
        assert_eq!(world.resource::<PendingCommandBuffers>().len(), 1);
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn pipelined_surface_work_returns_to_the_ui_owner_on_each_frame() {
        let owner = thread::current().id();
        let executor = MainThreadExecutor::new();
        let mut app = App::new();
        // PipelinedRenderingPlugin installs this before schedule configuration,
        // then shares it with the render world when it starts the render thread.
        app.insert_resource(executor.clone());
        let mut render_app = SubApp::new();
        render_app.update_schedule = Some(Render.intern());
        render_app
            .add_schedule(Schedule::new(Render))
            .init_resource::<ExecutionThreads>()
            .init_resource::<SubmissionThreads>()
            .add_systems(Render, (record_thread, record_submission));
        app.insert_sub_app(RenderApp, render_app);
        configure_frame_schedule_executors(&mut app);
        let mut render_app = app.remove_sub_app(RenderApp).unwrap();
        render_app.world_mut().insert_resource(executor.clone());
        let worker = thread::spawn(move || {
            let render_thread = thread::current().id();
            render_app.update();
            render_app.update();
            (
                render_app.world().resource::<ExecutionThreads>().0.clone(),
                render_app.world().resource::<SubmissionThreads>().0.clone(),
                render_thread,
            )
        });
        let ticker = executor.0.ticker().unwrap();
        while !worker.is_finished() {
            ticker.try_tick();
            thread::yield_now();
        }
        let (surface_threads, submission_threads, render_thread) = worker.join().unwrap();
        assert_eq!(surface_threads, [owner, owner]);
        assert_ne!(render_thread, owner);
        assert_eq!(submission_threads, [render_thread, render_thread]);
    }
}
