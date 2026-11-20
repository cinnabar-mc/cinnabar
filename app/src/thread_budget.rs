//! Process thread budget. World streaming sizes its own pool in chunk-pipeline; these
//! pools stay small so the frame and world workers are not oversubscribed. Every pool reads
//! the machine from the process's one CPU plan in `cpu-topology`.
//!
//! Threads fall into two groups. A frame thread can wait on Bevy's compute pool, the global
//! rayon pool and the world-stream service, and on async-compute threads too: mobile drivers
//! compile one pipeline at a time, so a pipeline the render thread creates waits behind one
//! compiling there. Those run on any core, and compute threads share the frame threads'
//! raised priority. Nothing waits on io tasks or world workers; they keep off the frame cores
//! the plan reserves.

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};

#[cfg(any(target_os = "android", test))]
mod android;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "android")]
use android as frame_priority;
#[cfg(target_os = "macos")]
use macos as frame_priority;

/// Thread counts for each shared pool on a machine with `cores` logical processors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThreadBudget {
    /// Transparent sorts, off-thread drops, and helpers for parallel work a frame thread
    /// shares and then waits on.
    pub(crate) rayon: usize,
    /// Scoped parallel work the main and render threads start and wait for.
    pub(crate) compute: usize,
    /// Pipeline compilation and other polled jobs.
    pub(crate) async_compute: usize,
    pub(crate) io: usize,
}

impl ThreadBudget {
    pub(crate) fn for_cores(cores: usize) -> Self {
        Self {
            rayon: (cores / 4).clamp(1, 2),
            compute: (cores / 2).clamp(2, 4),
            async_compute: if cores > 4 { 2 } else { 1 },
            io: 1,
        }
    }

    /// The budget for this machine. Where the CPU plan reserves frame cores, these pools size
    /// for the cores' combined speed instead of their count.
    fn current() -> Self {
        Self::for_cores(cpu_topology::plan().shared_pool_cores())
    }

    pub(crate) fn task_pool_plugin() -> TaskPoolPlugin {
        let budget = Self::current();
        let fixed = |threads| TaskPoolThreadAssignmentPolicy {
            min_threads: threads,
            max_threads: threads,
            percent: 1.0,
            on_thread_spawn: None,
            on_thread_destroy: None,
        };
        TaskPoolPlugin {
            task_pool_options: TaskPoolOptions {
                io: TaskPoolThreadAssignmentPolicy {
                    on_thread_spawn: Some(std::sync::Arc::new(
                        cpu_topology::enter_background_cores,
                    )),
                    ..fixed(budget.io)
                },
                async_compute: fixed(budget.async_compute),
                compute: TaskPoolThreadAssignmentPolicy {
                    #[cfg(any(target_os = "macos", target_os = "android"))]
                    on_thread_spawn: Some(std::sync::Arc::new(
                        frame_priority::prepare_frame_thread,
                    )),
                    ..fixed(budget.compute)
                },
                ..TaskPoolOptions::default()
            },
        }
    }

    /// Raises frame submission on the render executor's own thread. Android also raises the main
    /// schedule's thread, which the platform starts at normal priority; threads either one spawns
    /// later start at normal priority. Android also keeps background threads confined to the
    /// CPU plan's background cores when the plan reserves frame cores.
    #[cfg(any(target_os = "macos", target_os = "android"))]
    pub(crate) fn configure_frame_threads(app: &mut bevy::prelude::App) {
        use bevy::{
            ecs::schedule::common_conditions::run_once,
            prelude::IntoScheduleConfigs,
            render::{Render, RenderApp, RenderSystems},
        };
        #[cfg(target_os = "android")]
        app.add_systems(bevy::app::Startup, android::prepare_schedule_thread);
        #[cfg(target_os = "android")]
        if cpu_topology::plan().placement().is_some() {
            app.add_systems(
                bevy::app::Last,
                cpu_topology::refresh_background_threads.run_if(
                    bevy::time::common_conditions::on_real_timer(
                        android::BACKGROUND_CONFINEMENT_REFRESH,
                    ),
                ),
            );
        }
        #[cfg(target_os = "macos")]
        let prepare_render = macos::prepare_render_thread;
        #[cfg(target_os = "android")]
        let prepare_render = android::prepare_schedule_thread;
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(
                Render,
                prepare_render
                    .run_if(run_once)
                    .before(RenderSystems::ExtractCommands),
            );
        }
    }

    /// Must run before anything touches the global rayon pool. Logs the CPU plan and every
    /// pool's size once. Later calls, such as `run` after a setup-activity preload, do nothing.
    pub(crate) fn configure_global_rayon() {
        static CONFIGURED: std::sync::Once = std::sync::Once::new();
        CONFIGURED.call_once(|| {
            let budget = Self::current();
            bevy::log::info!(
                "cpu: {}; threads: world {}, rayon {}, compute {}, async-compute {}, io {}",
                cpu_topology::plan(),
                chunk_pipeline::world_worker_threads(),
                budget.rayon,
                budget.compute,
                budget.async_compute,
                budget.io,
            );
            if let Err(error) = rayon::ThreadPoolBuilder::new()
                .num_threads(budget.rayon)
                .thread_name(|index| format!("rayon-{index}"))
                .build_global()
            {
                bevy::log::warn!("global rayon pool was already configured: {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared pools stay small at every gate tier instead of scaling with the core count.
    #[test]
    fn shared_pools_are_sized_for_4_8_and_12_cores() {
        let budget = |rayon, compute, async_compute| ThreadBudget {
            rayon,
            compute,
            async_compute,
            io: 1,
        };
        assert_eq!(ThreadBudget::for_cores(4), budget(1, 2, 1));
        assert_eq!(ThreadBudget::for_cores(8), budget(2, 4, 2));
        assert_eq!(ThreadBudget::for_cores(12), budget(2, 4, 2));
        assert_eq!(ThreadBudget::for_cores(1), budget(1, 2, 1));
    }

    /// An eight-core phone whose cores add up to five or seven fast ones runs fewer raised
    /// compute threads and one rayon thread.
    #[test]
    fn shared_pools_shrink_with_a_phones_combined_core_speed() {
        let budget = |rayon, compute, async_compute| ThreadBudget {
            rayon,
            compute,
            async_compute,
            io: 1,
        };
        assert_eq!(ThreadBudget::for_cores(5), budget(1, 2, 2));
        assert_eq!(ThreadBudget::for_cores(7), budget(1, 3, 2));
    }
}
