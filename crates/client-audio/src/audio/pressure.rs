//! Audio's response to host memory pressure and to the app leaving the screen.

use bevy::{
    prelude::{MessageReader, NonSendMut, ResMut},
    window::AppLifecycle,
};
use memory_pressure::MemoryPressure;

use super::AudioEngine;
use crate::named_audio::AudioDevice;

/// Releases decoded sounds for the strongest pressure published this frame. Playing voices keep
/// their samples, and a released sound decodes again when it is next played.
pub fn release_sound_memory(
    mut pressure: MessageReader<MemoryPressure>,
    mut engine: ResMut<AudioEngine>,
) {
    let Some(pressure) = pressure.read().copied().max() else {
        return;
    };
    let released = engine.release_decoded(pressure);
    bevy::log::debug!(?pressure, released, "released decoded sounds");
}

/// Closes the audio output while the app is off screen and reopens it when it returns, so a
/// hidden client neither plays unheard nor keeps the audio hardware awake.
pub fn suspend_audio_output(
    mut lifecycle: MessageReader<AppLifecycle>,
    mut device: Option<NonSendMut<AudioDevice>>,
) {
    for change in lifecycle.read() {
        let suspended = match change {
            AppLifecycle::WillSuspend | AppLifecycle::Suspended => true,
            AppLifecycle::WillResume | AppLifecycle::Running => false,
            AppLifecycle::Idle => continue,
        };
        if let Some(device) = device.as_deref_mut() {
            device.set_suspended(suspended);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{App, Update};
    use memory_pressure::MemoryPressurePlugin;

    /// An app running both audio responses the way the client schedules them.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MemoryPressurePlugin)
            .add_systems(Update, (release_sound_memory, suspend_audio_output));
        app
    }

    /// Leaving the screen is the client's last frame until it returns, so that frame must
    /// already have dropped the decoded sounds, without any report from the host.
    #[test]
    fn leaving_the_screen_releases_decoded_sounds_in_the_same_frame() {
        let mut app = app();
        app.insert_resource(super::super::engine::tests::engine(&[("click", "ui")]));
        app.update();
        let cached = |app: &App| {
            app.world()
                .resource::<AudioEngine>()
                .bank()
                .expect("test bank")
                .cached_bytes()
        };
        assert!(
            cached(&app) > 0,
            "a running client keeps its decoded sounds"
        );

        app.world_mut().write_message(AppLifecycle::Suspended);
        app.update();
        assert_eq!(cached(&app), 0);
    }

    #[test]
    fn published_pressure_reaches_the_sound_cache() {
        let mut app = app();
        app.insert_resource(super::super::engine::tests::engine(&[("click", "ui")]));
        app.update();
        app.world_mut().write_message(MemoryPressure::Critical);
        app.update();
        let engine = app.world().resource::<AudioEngine>();
        assert_eq!(engine.bank().expect("test bank").cached_bytes(), 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_output_is_suspended_only_while_the_app_is_off_screen() {
        let mut app = app();
        app.init_resource::<AudioEngine>()
            .insert_non_send(AudioDevice::disabled());
        let suspended = |app: &App| app.world().non_send::<AudioDevice>().is_suspended();
        for (change, expected) in [
            (AppLifecycle::Running, false),
            (AppLifecycle::Suspended, true),
            (AppLifecycle::Running, false),
        ] {
            app.world_mut().write_message(change);
            app.update();
            assert_eq!(suspended(&app), expected, "{change:?}");
        }
    }
}
