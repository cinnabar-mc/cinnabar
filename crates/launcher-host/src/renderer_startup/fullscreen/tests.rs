use super::*;
use crate::video_settings::SavedVideoSettings;

/// Writes a healthy fullscreen preference with another setting that recovery must preserve.
fn saved(root: &Path) -> SavedVideoSettings {
    let settings = SavedVideoSettings {
        fullscreen: true,
        gui_scale_offset: 2,
    };
    video_settings::save(root, settings).unwrap();
    settings
}

#[test]
fn interrupted_fullscreen_entry_recovers_windowed_without_excluding_the_backend() {
    let dir = tempfile::tempdir().unwrap();
    let settings = saved(dir.path());
    let mut startup = super::super::StartupGuard::open(dir.path()).unwrap();
    startup
        .begin(super::super::Attempt {
            backend: launcher::graphics_backend::Backend::Vulkan,
            adapter: None,
            explicit: false,
            stage: super::super::Stage::FirstPresentation,
        })
        .unwrap();
    startup.presented().unwrap();
    let mut guard = FullscreenGuard::open(dir.path()).unwrap();
    guard.begin().unwrap();
    drop(guard); // Release the lease as an unhandled process exit would.
    let next = FullscreenGuard::open(dir.path()).unwrap();
    assert!(next.recovered);
    assert_eq!(
        video_settings::load(dir.path()).unwrap(),
        SavedVideoSettings {
            fullscreen: false,
            ..settings
        }
    );
    let renderer = super::super::StartupGuard::open(dir.path()).unwrap();
    assert!(renderer.recovery.failed_backends().is_empty());
    assert!(renderer.recovery.working.is_some());
}

#[test]
fn successful_entry_controlled_exit_and_unrelated_crash_preserve_fullscreen() {
    for entered in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let settings = saved(dir.path());
        let mut guard = FullscreenGuard::open(dir.path()).unwrap();
        if entered {
            guard.begin().unwrap();
            guard.clear().unwrap();
        }
        drop(guard);
        assert!(!FullscreenGuard::open(dir.path()).unwrap().recovered);
        assert_eq!(video_settings::load(dir.path()).unwrap(), settings);
    }
}

#[test]
fn a_live_transition_cannot_be_consumed_by_another_client() {
    let dir = tempfile::tempdir().unwrap();
    let settings = saved(dir.path());
    let mut guard = FullscreenGuard::open(dir.path()).unwrap();
    guard.begin().unwrap();
    assert!(FullscreenGuard::open(dir.path()).is_err());
    assert_eq!(video_settings::load(dir.path()).unwrap(), settings);
    guard.clear().unwrap();
}
