//! Channel changes preserve the user's check preference and bind older-build consent to one handoff.

use super::*;
use update_manifest::build::Build;

/// Changing the quit-time preference leaves a channel download and its progress running.
#[test]
fn quit_time_preference_changes_do_not_cancel_an_explicit_download() {
    let mut state = State::new();
    state.preferences.checks = false;
    state.running = true;
    let generation = state.generation;
    state.download_progress(generation, 25, 100);
    let download = state.download;
    assert_eq!(
        state.configure(Preferences {
            install_on_quit: true,
            ..state.preferences
        }),
        None
    );
    assert_eq!(state.generation, generation);
    assert_eq!(state.status, Status::Downloading);
    assert_eq!(state.download, download);
    assert!(state.preferences.install_on_quit);
    assert!(!state.preferences.checks);
}

/// An intentional channel change requests fresh metadata even with automatic checks off.
#[test]
fn changing_channels_requests_a_manual_check_without_enabling_automatic_checks() {
    let mut state = State::new();
    state.startup = StartupPhase::Complete;
    state.running = true;
    let previous = state.generation;
    state.download_progress(previous, 50, 100);
    state.configure(Preferences {
        channel: Channel::Nightly,
        checks: false,
        ..state.preferences
    });
    assert!(state.manual_check && state.check_now);
    assert!(!state.preferences.checks);
    assert!(state.download.is_none());
    state.download_progress(previous, 90, 100);
    assert!(state.download.is_none());
    assert!(state.finish_check(previous, false, true));
    assert!(state.manual_check && state.check_now);
    assert!(!state.preferences.checks);
}

/// Restoring a saved channel at launch does not turn an opt-out into a manual check.
#[test]
fn restoring_the_saved_channel_respects_disabled_automatic_checks() {
    let mut state = State::new();
    state.configure(Preferences {
        channel: Channel::Nightly,
        checks: false,
        ..state.preferences
    });
    assert!(!state.manual_check);
    assert!(!state.check_now);
}

/// Waiting changes neither the selected subscription nor installation consent.
#[test]
fn waiting_for_a_newer_release_keeps_the_older_release_available_by_choice() {
    let mut state = State::new();
    worker::offer_switch(&mut state);
    let preferences = state.preferences;
    state.wait_for_channel();
    assert!(state.waiting_for_channel);
    assert!(state.switch_available);
    assert!(!state.switch_now && !state.requested);
    assert_eq!(state.preferences, preferences);
    assert!(state.request_switch());
    assert!(state.switch_now);
    assert!(!state.waiting_for_channel);
    state.configure(Preferences {
        channel: Channel::Nightly,
        ..preferences
    });
    assert!(!state.switch_now && !state.waiting_for_channel);
}

/// A resumed transfer reports its real bytes and stale workers cannot advance it.
#[test]
fn resumed_download_measurements_belong_to_the_active_channel() {
    let mut state = State::new();
    state.download_progress(state.generation, 25, 100);
    assert_eq!(state.download.unwrap().percent(), Some(25));
    state.download_progress(state.generation, 60, 100);
    assert_eq!(state.download.unwrap().percent(), Some(60));
    let previous = state.generation;
    state.configure(Preferences {
        channel: Channel::Nightly,
        ..state.preferences
    });
    state.download_progress(previous, 100, 100);
    assert!(state.download.is_none());
}

/// Only Restart and switch sends older-build consent to the helper; quit-time opt-in is insufficient.
#[test]
fn older_build_handoffs_include_the_exact_installed_identity() {
    let mut ready = tests::ready(true);
    let target = PathBuf::from("installed-client");
    assert_eq!(handoff::helper_args(&ready, target.clone(), 42).len(), 4);
    ready.switch_from = Some(Build {
        version: "3.0.0".into(),
        number: 7,
        channel: Some(Channel::Nightly),
    });
    ready.notice.switching = true;
    let args = handoff::helper_args(&ready, target, 42);
    assert_eq!(args[4], "3.0.0");
    assert_eq!(args[5], "7");
    let mut state = State::new();
    state.preferences.install_on_quit = true;
    state.ready = Some(ready);
    assert!(!state.applies_on_exit(OffsetDateTime::UNIX_EPOCH));
    assert!(state.request_restart(OffsetDateTime::UNIX_EPOCH));
    assert!(
        state
            .handoff_after_exit(OffsetDateTime::UNIX_EPOCH)
            .is_some()
    );
}
