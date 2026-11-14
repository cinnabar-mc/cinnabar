//! Asks for a memory line at the points of a session where memory use changes: the first menu
//! frame, a join's start, world entry and exit, and when the platform takes the app off screen
//! or brings it back. It also keeps the subsystem totals fresh for the `periodic` lines the
//! reporter logs on its own. The lines themselves are read and logged by
//! [`launcher_host::memory::telemetry`], off the frame.

use std::time::Duration;

use bevy::{ecs::system::SystemParam, prelude::*, time::Real, window::AppLifecycle};
use launcher_host::memory::{snapshot::Counters, telemetry};
use render_scene::{
    ActorArtworkResource, ChunkRenderQueue, UiRenderStatsResource,
    startup_presentation::StartupPresentation,
};

use crate::{runtime::world::ClientWorld, session::SessionController};

/// How often the subsystem totals behind the reporter's `periodic` lines are refreshed.
const COUNTER_REFRESH: Duration = Duration::from_secs(10);
/// Frames between an event and its line: the render world samples the device allocator at the
/// end of the frame after the request, which the main world sees one frame later.
const GPU_SAMPLE_FRAMES: u8 = 2;

/// Reports memory at session milestones and platform lifecycle changes.
pub(crate) fn configure(app: &mut App) {
    telemetry::platform_ready();
    telemetry::add_child_source(shaderpack::worker::live_process_ids);
    app.add_message::<AppLifecycle>()
        .add_systems(Last, report_memory);
}

/// What one frame shows of the client's progress.
#[derive(Clone, Copy, Default)]
struct Observed {
    /// The menu's first content frame has reached the screen.
    menu_presented: bool,
    /// A join is in progress.
    connecting: bool,
    /// A world is loaded.
    in_world: bool,
}

/// The milestones already reported.
#[derive(Default)]
struct Milestones {
    menu_ready: bool,
    connecting: bool,
    in_world: bool,
}

impl Milestones {
    /// Calls `emit` with each event `observed` calls for.
    fn observe(&mut self, observed: Observed, mut emit: impl FnMut(&'static str)) {
        if observed.menu_presented && !self.menu_ready {
            self.menu_ready = true;
            emit("menu_ready");
        }
        if observed.connecting && !self.connecting && !observed.in_world {
            emit("join_start");
        }
        self.connecting = observed.connecting;
        match (self.in_world, observed.in_world) {
            (false, true) => emit("world_entry"),
            (true, false) => emit("world_exit"),
            _ => {}
        }
        self.in_world = observed.in_world;
    }
}

/// Whether the subsystem totals are due for a refresh at `now`, a time on any steady clock.
fn refresh_due(last: Option<Duration>, now: Duration) -> bool {
    last.is_none_or(|last| now.saturating_sub(last) >= COUNTER_REFRESH)
}

/// The event a platform lifecycle change calls for. Bevy runs one last frame for
/// `WillSuspend` and none until the app returns, so that frame reports at once.
fn lifecycle_event(lifecycle: AppLifecycle) -> Option<&'static str> {
    match lifecycle {
        AppLifecycle::WillSuspend => Some("background"),
        AppLifecycle::WillResume => Some("foreground"),
        AppLifecycle::Idle | AppLifecycle::Running | AppLifecycle::Suspended => None,
    }
}

/// Events waiting for the device allocator's sample.
#[derive(Default)]
struct Pending(Vec<(&'static str, u8)>);

impl Pending {
    /// Queues `event` for [`GPU_SAMPLE_FRAMES`] frames from now.
    fn defer(&mut self, event: &'static str) {
        self.0.push((event, GPU_SAMPLE_FRAMES));
    }

    /// Ends one frame, calling `send` with the events whose wait is over; `flush` ends every
    /// wait, for the last frame before the app leaves the screen.
    fn end_frame(&mut self, flush: bool, mut send: impl FnMut(&'static str)) {
        self.0.retain_mut(|(event, frames)| {
            *frames = frames.saturating_sub(1);
            let ready = flush || *frames == 0;
            if ready {
                send(event);
            }
            !ready
        });
    }
}

/// The subsystem totals reported beside the kernel's figures.
#[derive(SystemParam)]
struct Sources<'w> {
    world: Option<Res<'w, ClientWorld>>,
    ui: Option<Res<'w, UiRenderStatsResource>>,
    actor_art: Option<Res<'w, ActorArtworkResource>>,
    chunk_queue: Option<Res<'w, ChunkRenderQueue>>,
}

impl Sources<'_> {
    /// Reads the totals their owners already keep; nothing here walks world or mesh data.
    fn counters(&self) -> Counters {
        let mut counters = Counters::default();
        let mut pool = |name, bytes: Option<u64>| {
            if let Some(bytes) = bytes {
                counters.pools.push((name, bytes));
            }
        };
        let gpu = render::memory_totals::gpu_totals();
        pool("gpu_allocated", gpu.map(|gpu| gpu.allocated));
        pool("gpu_reserved", gpu.map(|gpu| gpu.reserved));
        pool("chunk_arena", render::memory_totals::chunk_arena_bytes());
        pool(
            "ui_gpu",
            self.ui
                .as_ref()
                .map(|ui| ui.0.snapshot().retained_gpu_bytes),
        );
        pool(
            "actor_art",
            self.actor_art.as_ref().map(|art| {
                art.0
                    .pages()
                    .iter()
                    .map(|page| page.rgba8.len() as u64)
                    .sum()
            }),
        );
        pool(
            "chunk_uploads_pending",
            self.chunk_queue.as_ref().map(|queue| queue.pending_bytes()),
        );
        pool(
            "kept_packs",
            Some(crate::runtime::network::compiled_stacks().archive_bytes() as u64),
        );
        if let Some(stream) = self.world.as_ref().and_then(|world| world.stream.as_ref()) {
            counters
                .counts
                .push(("sections", stream.stats().resident_sub_chunks as u64));
        }
        counters
    }
}

/// What the reporting system remembers between frames.
#[derive(Default)]
struct ReportState {
    milestones: Milestones,
    pending: Pending,
    /// When the subsystem totals were last refreshed.
    refreshed_at: Option<Duration>,
}

/// Watches the session and the platform lifecycle, and asks for the lines they call for.
fn report_memory(
    time: Res<Time<Real>>,
    presentation: Option<Res<StartupPresentation>>,
    session: Option<Res<SessionController>>,
    sources: Sources,
    mut lifecycle: MessageReader<AppLifecycle>,
    mut state: Local<ReportState>,
) {
    let ReportState {
        milestones,
        pending,
        refreshed_at,
    } = &mut *state;
    let observed = Observed {
        menu_presented: presentation.is_some_and(|presentation| presentation.is_complete()),
        connecting: session.is_some_and(|session| session.status().connecting),
        in_world: sources
            .world
            .as_ref()
            .is_some_and(|world| world.stream.is_some()),
    };
    let mut leaving = false;
    let mut events = Vec::new();
    milestones.observe(observed, |event| events.push(event));
    for change in lifecycle.read() {
        leaving |= *change == AppLifecycle::WillSuspend;
        events.extend(lifecycle_event(*change));
    }
    let now = time.elapsed();
    let refresh = refresh_due(*refreshed_at, now);
    if refresh || !events.is_empty() {
        render::memory_totals::request_gpu_totals();
    }
    for event in events {
        pending.defer(event);
    }
    let mut ready = Vec::new();
    pending.end_frame(leaving, |event| ready.push(event));
    if refresh || !ready.is_empty() {
        *refreshed_at = Some(now);
        telemetry::set_counters(sources.counters());
    }
    for event in ready {
        telemetry::report(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: Observed = Observed {
        menu_presented: true,
        connecting: false,
        in_world: false,
    };
    const JOINING: Observed = Observed {
        menu_presented: true,
        connecting: true,
        in_world: false,
    };
    const WORLD: Observed = Observed {
        menu_presented: true,
        connecting: false,
        in_world: true,
    };

    /// The events `milestones` emits for `observed`.
    fn observe(milestones: &mut Milestones, observed: Observed) -> Vec<&'static str> {
        let mut events = Vec::new();
        milestones.observe(observed, |event| events.push(event));
        events
    }

    #[test]
    fn a_session_reports_each_milestone_once() {
        let mut milestones = Milestones::default();
        assert!(observe(&mut milestones, Observed::default()).is_empty());
        assert_eq!(observe(&mut milestones, MENU), ["menu_ready"]);
        assert!(observe(&mut milestones, MENU).is_empty());
        assert_eq!(observe(&mut milestones, JOINING), ["join_start"]);
        assert!(observe(&mut milestones, JOINING).is_empty());
        assert_eq!(observe(&mut milestones, WORLD), ["world_entry"]);
        assert!(observe(&mut milestones, WORLD).is_empty());
        assert_eq!(observe(&mut milestones, MENU), ["world_exit"]);
        assert!(observe(&mut milestones, MENU).is_empty());
        assert_eq!(observe(&mut milestones, JOINING), ["join_start"]);
        assert_eq!(observe(&mut milestones, WORLD), ["world_entry"]);
    }

    #[test]
    fn a_world_entered_before_the_menu_frame_reports_both() {
        let mut milestones = Milestones::default();
        assert_eq!(
            observe(&mut milestones, WORLD),
            ["menu_ready", "world_entry"]
        );
    }

    #[test]
    fn a_transfer_between_worlds_is_not_a_new_join() {
        let mut milestones = Milestones::default();
        observe(&mut milestones, WORLD);
        let transferring = Observed {
            connecting: true,
            ..WORLD
        };
        assert!(observe(&mut milestones, transferring).is_empty());
    }

    #[test]
    fn totals_refresh_on_the_first_frame_and_then_every_interval() {
        let at = Duration::from_secs;
        assert!(refresh_due(None, at(0)));
        assert!(!refresh_due(Some(at(5)), at(5) + COUNTER_REFRESH - at(1)));
        assert!(refresh_due(Some(at(5)), at(5) + COUNTER_REFRESH));
    }

    #[test]
    fn only_leaving_and_returning_are_lifecycle_events() {
        assert_eq!(
            lifecycle_event(AppLifecycle::WillSuspend),
            Some("background")
        );
        assert_eq!(
            lifecycle_event(AppLifecycle::WillResume),
            Some("foreground")
        );
        for quiet in [
            AppLifecycle::Idle,
            AppLifecycle::Running,
            AppLifecycle::Suspended,
        ] {
            assert_eq!(lifecycle_event(quiet), None);
        }
    }

    /// The events `pending` sends at the end of one frame.
    fn end_frame(pending: &mut Pending, flush: bool) -> Vec<&'static str> {
        let mut sent = Vec::new();
        pending.end_frame(flush, |event| sent.push(event));
        sent
    }

    #[test]
    fn an_event_waits_for_the_allocator_sample() {
        let mut pending = Pending::default();
        pending.defer("world_entry");
        for _ in 1..GPU_SAMPLE_FRAMES {
            assert!(end_frame(&mut pending, false).is_empty());
        }
        assert_eq!(end_frame(&mut pending, false), ["world_entry"]);
        assert!(end_frame(&mut pending, false).is_empty());
    }

    #[test]
    fn the_last_frame_before_leaving_sends_everything_waiting() {
        let mut pending = Pending::default();
        pending.defer("world_exit");
        pending.defer("background");
        assert_eq!(end_frame(&mut pending, true), ["world_exit", "background"]);
        assert!(pending.0.is_empty());
    }
}
