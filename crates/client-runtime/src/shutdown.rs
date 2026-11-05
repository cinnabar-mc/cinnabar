//! Bounded process-shutdown watchdog shared by startup and teardown.

use std::{
    io::Write,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU8, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use bevy::{
    app::AppExit,
    ecs::system::NonSendMarker,
    prelude::{Entity, MessageReader, Query, Res, Resource},
    window::Window,
    winit::WINIT_WINDOWS,
};

use diagnostics::markers::{
    SHUTDOWN_PHASE_MARKER, SHUTDOWN_WATCHDOG_ARMED_MARKER, SHUTDOWN_WATCHDOG_FIRED_MARKER,
};

pub const SHUTDOWN_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(2);

pub const SHUTDOWN_WATCHDOG_IDLE: u8 = 0;
pub const SHUTDOWN_WATCHDOG_ARMED: u8 = 1;
pub const SHUTDOWN_WATCHDOG_COMPLETED: u8 = 2;
pub const SHUTDOWN_WATCHDOG_FIRED: u8 = 3;

pub type ShutdownTerminator = Arc<dyn Fn(i32) + Send + Sync + 'static>;

#[derive(Resource, Clone)]
pub struct ShutdownWatchdog {
    pub state: Arc<AtomicU8>,
    pub timeout: Duration,
    pub terminate: ShutdownTerminator,
}

impl ShutdownWatchdog {
    /// Creates a watchdog that ends the client as `main` would after a finished teardown.
    pub fn process(timeout: Duration) -> Self {
        Self::new(timeout, |code| {
            end_cut_short(
                code,
                || launcher_host::lifecycle::children::stop_all(Duration::ZERO),
                launcher_host::lifecycle::update::apply_after_exit,
                launcher_host::lifecycle::crash::announce_exit,
            );
            std::process::exit(code)
        })
    }

    /// Creates a watchdog with a caller-supplied termination action.
    pub fn new<F>(timeout: Duration, terminate: F) -> Self
    where
        F: Fn(i32) + Send + Sync + 'static,
    {
        Self {
            state: Arc::new(AtomicU8::new(SHUTDOWN_WATCHDOG_IDLE)),
            timeout,
            terminate: Arc::new(terminate),
        }
    }

    /// Starts the timeout once and returns whether this call armed it.
    pub fn arm(&self, exit: AppExit) -> bool {
        if self
            .state
            .compare_exchange(
                SHUTDOWN_WATCHDOG_IDLE,
                SHUTDOWN_WATCHDOG_ARMED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return false;
        }
        let state = Arc::clone(&self.state);
        let terminate = Arc::clone(&self.terminate);
        let timeout = self.timeout;
        let exit_code = app_exit_code(&exit);
        let spawned = thread::Builder::new()
            .name("bedrock-shutdown-watchdog".to_owned())
            .spawn(move || {
                thread::sleep(timeout);
                if state
                    .compare_exchange(
                        SHUTDOWN_WATCHDOG_ARMED,
                        SHUTDOWN_WATCHDOG_FIRED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let _ = writeln!(
                        diagnostics::console::stderr(),
                        "{SHUTDOWN_WATCHDOG_FIRED_MARKER} timeout_ms={} exit_code={exit_code}",
                        timeout.as_millis()
                    );
                    diagnostics::console::flush_before_exit();
                    terminate(exit_code);
                }
            });
        if spawned.is_err() {
            self.state.store(SHUTDOWN_WATCHDOG_FIRED, Ordering::Release);
            diagnostics::console::flush_before_exit();
            (self.terminate)(exit_code);
        }
        true
    }

    /// Marks shutdown complete so an armed timeout cannot terminate the process.
    pub fn complete(&self) {
        self.state
            .store(SHUTDOWN_WATCHDOG_COMPLETED, Ordering::Release);
    }
}

/// Runs the exit steps a teardown cut short by the watchdog would otherwise skip: `stop` ends
/// owned children, a successful exit still runs `hand_off_update` so an accepted update reaches
/// its helper, and `announce` reports the status to the crash monitor last.
fn end_cut_short(
    code: i32,
    stop: impl FnOnce(),
    hand_off_update: impl FnOnce(),
    announce: impl FnOnce(i32),
) {
    stop();
    if code == 0 {
        hand_off_update();
    }
    announce(code);
}

/// Arms the watchdog when the world is torn down without an `AppExit`, as macOS `terminate:`
/// (Cmd+Q, Dock Quit) and a panic unwinding the main thread do; Bevy's pipelined-render teardown
/// otherwise deadlocks there. Insert before the render plugins finish so it drops ahead of
/// `RenderAppChannels`.
#[derive(Resource)]
pub struct TeardownWatchdog(pub ShutdownWatchdog);

/// The status Rust exits with after a panic on the main thread.
const PANIC_EXIT_CODE: u8 = launcher_host::lifecycle::crash::protocol::PANIC_EXIT_CODE;

impl Drop for TeardownWatchdog {
    fn drop(&mut self) {
        // A watchdog cutting short a panic's teardown must still end the process as failed.
        let exit = if thread::panicking() {
            AppExit::from_code(PANIC_EXIT_CODE)
        } else {
            AppExit::Success
        };
        begin_bounded_shutdown(&self.0, &exit);
        log_shutdown_phase("world_teardown_begin");
    }
}

/// Converts the Bevy exit reason to its process exit status.
pub fn app_exit_code(exit: &AppExit) -> i32 {
    match exit {
        AppExit::Success => 0,
        AppExit::Error(code) => i32::from(code.get()),
    }
}

/// When shutdown began, so each teardown step can report how far into it the process is.
static SHUTDOWN_STARTED: OnceLock<Instant> = OnceLock::new();

/// Logs that a teardown step finished, with the time since shutdown began, so a slow exit can be
/// attributed to the step that spends it.
pub fn log_shutdown_phase(phase: &str) {
    let elapsed = SHUTDOWN_STARTED.get().map(|started| started.elapsed());
    let _ = writeln!(
        diagnostics::console::stderr(),
        "{SHUTDOWN_PHASE_MARKER} phase={phase} elapsed_ms={:.1}",
        elapsed.map_or(-1.0, |elapsed| elapsed.as_secs_f64() * 1000.0)
    );
}

/// Arms shutdown once and records the requested timeout and exit status.
pub fn begin_bounded_shutdown(watchdog: &ShutdownWatchdog, exit: &AppExit) {
    SHUTDOWN_STARTED.get_or_init(Instant::now);
    if watchdog.arm(exit.clone()) {
        let _ = writeln!(
            diagnostics::console::stderr(),
            "{SHUTDOWN_WATCHDOG_ARMED_MARKER} timeout_ms={} exit_code={}",
            watchdog.timeout.as_millis(),
            app_exit_code(exit)
        );
    }
}

/// Hides every window as soon as an exit is requested, so the teardown that follows (trace and
/// far-terrain flushes, network and GPU shutdown) never shows as a window that ignores its close
/// button. The request reaches winit directly because `changed_windows` runs unordered with the
/// blocking exit systems; the `Window` component is kept in step so it applies the same state.
/// Order every blocking exit system after this one.
pub fn hide_windows_on_exit(
    mut exits: MessageReader<AppExit>,
    mut windows: Query<(Entity, &mut Window)>,
    _main_thread: NonSendMarker,
) {
    if exits.read().next().is_none() {
        return;
    }
    for (entity, mut window) in &mut windows {
        window.visible = false;
        WINIT_WINDOWS.with_borrow(|winit| {
            if let Some(native) = winit.get_window(entity) {
                native.set_visible(false);
            }
        });
    }
}

/// Arms the watchdog for exit requests, preserving any failure status.
pub fn arm_shutdown_watchdog(mut exits: MessageReader<AppExit>, watchdog: Res<ShutdownWatchdog>) {
    let requested = exits.read().cloned().reduce(
        |selected, next| {
            if selected.is_error() { selected } else { next }
        },
    );
    if let Some(exit) = requested {
        begin_bounded_shutdown(&watchdog, &exit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records whether the window was already hidden when a blocking exit system ran.
    #[derive(Resource, Default)]
    struct HiddenBeforeBlocking(Option<bool>);

    fn record_window_visibility(
        windows: Query<&Window>,
        mut seen: bevy::prelude::ResMut<HiddenBeforeBlocking>,
    ) {
        seen.0 = windows.iter().next().map(|window| !window.visible);
    }

    fn exit_test_app() -> bevy::prelude::App {
        use bevy::prelude::{App, IntoScheduleConfigs, Last};
        let mut app = App::new();
        app.add_message::<AppExit>()
            .init_resource::<HiddenBeforeBlocking>()
            .insert_resource(ShutdownWatchdog::new(Duration::from_secs(60), |_| {}))
            .add_systems(
                Last,
                (
                    (hide_windows_on_exit, arm_shutdown_watchdog).chain(),
                    record_window_visibility.after(arm_shutdown_watchdog),
                ),
            );
        app.world_mut().spawn(Window::default());
        app
    }

    #[test]
    fn a_requested_exit_hides_the_window_before_blocking_exit_work() {
        let mut app = exit_test_app();
        app.world_mut().write_message(AppExit::Success);
        app.update();
        assert_eq!(app.world().resource::<HiddenBeforeBlocking>().0, Some(true));
    }

    #[test]
    fn the_window_stays_visible_until_an_exit_is_requested() {
        let mut app = exit_test_app();
        app.update();
        assert_eq!(
            app.world().resource::<HiddenBeforeBlocking>().0,
            Some(false)
        );
    }

    fn cut_short(code: i32) -> Vec<String> {
        let steps = RefCell::new(Vec::new());
        end_cut_short(
            code,
            || steps.borrow_mut().push("stop".to_owned()),
            || steps.borrow_mut().push("hand off update".to_owned()),
            |code| steps.borrow_mut().push(format!("announce {code}")),
        );
        steps.into_inner()
    }

    #[test]
    fn a_successful_exit_cut_short_still_hands_off_an_accepted_update() {
        assert_eq!(cut_short(0), ["stop", "hand off update", "announce 0"]);
    }

    #[test]
    fn a_failed_exit_cut_short_never_installs_an_update() {
        assert_eq!(cut_short(1), ["stop", "announce 1"]);
        assert_eq!(
            cut_short(i32::from(PANIC_EXIT_CODE)),
            ["stop", format!("announce {PANIC_EXIT_CODE}").as_str()]
        );
    }
}
