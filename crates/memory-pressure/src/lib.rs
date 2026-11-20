//! Host memory pressure, as the message cache owners release rebuildable memory on.
//!
//! A platform edge reports what its OS says from any thread with [`report`].
//! [`MemoryPressurePlugin`] publishes at most one [`MemoryPressure`] per frame from those reports
//! and from the app leaving the screen. Owners read the message in their own systems and rebuild
//! what they released when it is next needed.

use std::sync::atomic::{AtomicU8, Ordering};

use bevy::{
    app::{App, First, Plugin},
    ecs::message::{Message, MessageReader, MessageWriter},
    prelude::Local,
    window::AppLifecycle,
};

/// How much memory the host wants back, ordered from the least to the most to release.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum MemoryPressure {
    /// The device is running low while the client is on screen: shrink caches, keeping what the
    /// next frames are likely to reuse.
    Low = 1,
    /// The device is critically low while the client is on screen: drop every cache that can be
    /// rebuilt, keeping work already in flight for the current frames.
    Critical = 2,
    /// The client left the screen: drop every rebuildable cache and abandon queued work. Nothing
    /// is needed until it returns, and the largest hidden process is reclaimed first.
    Background = 3,
}

/// Android `ComponentCallbacks2` trim levels at which each pressure starts. Android asks callers
/// to compare against thresholds, because levels between and above the named ones may appear.
mod android_trim {
    /// `TRIM_MEMORY_RUNNING_LOW`
    pub(super) const RUNNING_LOW: i32 = 10;
    /// `TRIM_MEMORY_RUNNING_CRITICAL`
    pub(super) const RUNNING_CRITICAL: i32 = 15;
    /// `TRIM_MEMORY_UI_HIDDEN`; every higher level also describes a hidden process.
    pub(super) const UI_HIDDEN: i32 = 20;
}

impl MemoryPressure {
    /// The pressure an Android `onTrimMemory` level asks for. `TRIM_MEMORY_RUNNING_MODERATE`
    /// and lower levels ask for nothing.
    pub const fn from_android_trim_level(level: i32) -> Option<Self> {
        if level >= android_trim::UI_HIDDEN {
            Some(Self::Background)
        } else if level >= android_trim::RUNNING_CRITICAL {
            Some(Self::Critical)
        } else if level >= android_trim::RUNNING_LOW {
            Some(Self::Low)
        } else {
            None
        }
    }

    /// Whether owners should empty their caches instead of shrinking them.
    pub const fn releases_everything(self) -> bool {
        !matches!(self, Self::Low)
    }

    /// The pressure stored as `rank` by [`report`]; zero is no report.
    const fn from_rank(rank: u8) -> Option<Self> {
        match rank {
            1 => Some(Self::Low),
            2 => Some(Self::Critical),
            3 => Some(Self::Background),
            _ => None,
        }
    }
}

/// The strongest unpublished report, as a [`MemoryPressure`] discriminant; zero when empty.
static REPORTED: AtomicU8 = AtomicU8::new(0);

/// Records what the host OS reported, from any thread. The strongest report since the last
/// frame is published in the next one.
pub fn report(pressure: MemoryPressure) {
    REPORTED.fetch_max(pressure as u8, Ordering::AcqRel);
}

/// Takes the strongest report made since the previous call.
fn take_reported() -> Option<MemoryPressure> {
    MemoryPressure::from_rank(REPORTED.swap(0, Ordering::AcqRel))
}

/// Publishes [`MemoryPressure`] in `First`, so every later schedule of the same frame, including
/// render extraction, can release memory before a suspended app stops updating.
pub struct MemoryPressurePlugin;

impl Plugin for MemoryPressurePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AppLifecycle>()
            .add_message::<MemoryPressure>()
            .add_systems(First, publish_memory_pressure);
    }
}

/// What one frame's lifecycle changes and host report ask owners to release. `hidden` carries
/// whether the app was off screen after the previous frame.
fn frame_pressure(
    hidden: &mut bool,
    lifecycle: impl IntoIterator<Item = AppLifecycle>,
    reported: Option<MemoryPressure>,
) -> Option<MemoryPressure> {
    let was_hidden = *hidden;
    for change in lifecycle {
        match change {
            AppLifecycle::WillSuspend | AppLifecycle::Suspended => *hidden = true,
            AppLifecycle::WillResume | AppLifecycle::Running => *hidden = false,
            AppLifecycle::Idle => {}
        }
    }
    match (was_hidden, *hidden) {
        // The frame that leaves the screen is the last one until the app returns, and the
        // host's own report may arrive after it.
        (false, true) => Some(MemoryPressure::Background),
        // Reports made while hidden were answered when the app left the screen.
        (true, _) => None,
        // A hidden-process report can arrive just before the app leaves the screen, or late,
        // after it returned. A visible client keeps the work its current frames wait on.
        (false, false) => reported.map(|pressure| pressure.min(MemoryPressure::Critical)),
    }
}

/// Writes this frame's pressure, if any, for cache owners to read.
fn publish_memory_pressure(
    mut hidden: Local<bool>,
    mut lifecycle: MessageReader<AppLifecycle>,
    mut pressure: MessageWriter<MemoryPressure>,
) {
    let reported = take_reported();
    if let Some(frame) = frame_pressure(&mut hidden, lifecycle.read().copied(), reported) {
        bevy::log::info!(pressure = ?frame, "releasing rebuildable memory");
        pressure.write(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::message::Messages;

    #[test]
    fn android_trim_levels_map_by_threshold() {
        use MemoryPressure::{Background, Critical, Low};
        // The named `ComponentCallbacks2` levels, then values between and beyond them.
        for (level, expected) in [
            (5, None),
            (10, Some(Low)),
            (15, Some(Critical)),
            (20, Some(Background)),
            (40, Some(Background)),
            (60, Some(Background)),
            (80, Some(Background)),
            (i32::MIN, None),
            (0, None),
            (9, None),
            (12, Some(Low)),
            (19, Some(Critical)),
            (i32::MAX, Some(Background)),
        ] {
            assert_eq!(
                MemoryPressure::from_android_trim_level(level),
                expected,
                "{level}"
            );
        }
    }

    #[test]
    fn only_low_pressure_keeps_part_of_a_cache() {
        assert!(!MemoryPressure::Low.releases_everything());
        assert!(MemoryPressure::Critical.releases_everything());
        assert!(MemoryPressure::Background.releases_everything());
        assert!(MemoryPressure::Low < MemoryPressure::Critical);
        assert!(MemoryPressure::Critical < MemoryPressure::Background);
    }

    #[test]
    fn leaving_the_screen_releases_without_waiting_for_a_host_report() {
        let mut hidden = false;
        assert_eq!(
            frame_pressure(&mut hidden, [AppLifecycle::Running], None),
            None
        );
        assert_eq!(
            frame_pressure(&mut hidden, [AppLifecycle::Suspended], None),
            Some(MemoryPressure::Background)
        );
        assert!(hidden);
    }

    #[test]
    fn foreground_reports_pass_through_and_hidden_ones_are_not_replayed() {
        let mut hidden = false;
        assert_eq!(
            frame_pressure(&mut hidden, [], Some(MemoryPressure::Low)),
            Some(MemoryPressure::Low)
        );
        assert_eq!(
            frame_pressure(
                &mut hidden,
                [AppLifecycle::Suspended],
                Some(MemoryPressure::Critical)
            ),
            Some(MemoryPressure::Background)
        );
        // The host's hidden-process reports arrive while no frame runs; the returning frame
        // must not empty caches a second time.
        assert_eq!(
            frame_pressure(
                &mut hidden,
                [AppLifecycle::Running],
                Some(MemoryPressure::Background)
            ),
            None
        );
        assert!(!hidden);
        assert_eq!(
            frame_pressure(&mut hidden, [], Some(MemoryPressure::Critical)),
            Some(MemoryPressure::Critical)
        );
        // A hidden-process report that reaches a visible client arrived early or late.
        assert_eq!(
            frame_pressure(&mut hidden, [], Some(MemoryPressure::Background)),
            Some(MemoryPressure::Critical)
        );
    }

    /// The process-wide report slot reaches readers once, strongest report first.
    #[test]
    fn the_plugin_publishes_the_strongest_report_once() {
        let mut app = App::new();
        app.add_plugins(MemoryPressurePlugin);
        app.update();
        report(MemoryPressure::Critical);
        report(MemoryPressure::Low);
        app.update();
        let published: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<MemoryPressure>>()
            .drain()
            .collect();
        assert_eq!(published, [MemoryPressure::Critical]);
        app.update();
        assert!(
            app.world()
                .resource::<Messages<MemoryPressure>>()
                .is_empty()
        );
    }
}
