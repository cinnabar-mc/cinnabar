//! Bounded sound requests drained by the app audio adapter at its existing frame stage.

/// Sound the pack's ordinary JSON-UI buttons and toggles declare, at volume and pitch 1.
pub const UI_CLICK: &str = "random.click";

/// The press sound of a client-owned OreUI control, which has no pack definition to declare one.
pub fn native_press_sound() -> &'static json_ui::ControlSound {
    static CLICK: std::sync::LazyLock<json_ui::ControlSound> =
        std::sync::LazyLock::new(|| json_ui::ControlSound {
            name: UI_CLICK.into(),
            volume: 1.0,
            pitch: 1.0,
            min_seconds: 0.0,
        });
    &CLICK
}

/// Interface sounds JSON-UI sound components asked for: name, volume, pitch.
static PENDING_UI_SOUNDS: std::sync::Mutex<Vec<(String, f32, f32)>> =
    std::sync::Mutex::new(Vec::new());
/// Bounds one frame's queued control sounds.
const MAX_PENDING_UI_SOUNDS: usize = 16;

/// Plays a pressed launcher control's sound, holding back a repeat inside its
/// `min_seconds_between_plays`, as vanilla's sound component does.
pub fn ui_control_sound(sound: &json_ui::ControlSound) {
    static LAST_PLAYED: std::sync::Mutex<Vec<(String, web_time::Instant)>> =
        std::sync::Mutex::new(Vec::new());
    if sound.min_seconds > 0.0 {
        let mut played = LAST_PLAYED
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let now = web_time::Instant::now();
        match played.iter().position(|(name, _)| *name == sound.name) {
            Some(index)
                if now.duration_since(played[index].1).as_secs_f32() < sound.min_seconds =>
            {
                return;
            }
            Some(index) => played[index].1 = now,
            None if played.len() < MAX_PENDING_UI_SOUNDS => played.push((sound.name.clone(), now)),
            None => {}
        }
    }
    ui_sound(&sound.name, sound.volume, sound.pitch);
}

/// Requests an interface sound a UI sound component names, at its volume and pitch.
pub fn ui_sound(name: &str, volume: f32, pitch: f32) {
    let mut pending = PENDING_UI_SOUNDS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if pending.len() < MAX_PENDING_UI_SOUNDS {
        pending.push((name.to_owned(), volume, pitch));
    }
}

/// Takes the queued named sounds in their original request order.
pub fn take_sounds() -> Vec<(String, f32, f32)> {
    std::mem::take(
        &mut *PENDING_UI_SOUNDS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_clicks_reach_the_audio_queue_with_their_authored_scalars() {
        ui_sound(UI_CLICK, 0.75, 1.0);
        ui_control_sound(native_press_sound());
        ui_sound("ui.reject", 0.5, 1.25);
        let sounds = take_sounds();
        for expected in [
            (UI_CLICK, 0.75, 1.0),
            (UI_CLICK, 1.0, 1.0),
            ("ui.reject", 0.5, 1.25),
        ] {
            assert!(
                sounds
                    .iter()
                    .any(|(name, volume, pitch)| (name.as_str(), *volume, *pitch) == expected),
                "{expected:?} was dropped from {sounds:?}"
            );
        }
    }
}
