//! Ordered platform input and modifier observations for focused native mod fields.

use super::{Ime, PhysicalControls, WindowEvent};
use bevy::{input::keyboard::KeyboardInput, prelude::*};
use ui::mod_panel::FieldModifiers;

pub(super) type KeySample = (String, Option<String>, bool, FieldModifiers);

pub(super) enum PanelSample {
    Key(KeySample),
    Ime(Ime),
}

impl PanelSample {
    /// Exposes only physical key samples to panel toggle and reservation policy.
    pub fn key(&self) -> Option<&KeySample> {
        match self {
            Self::Key(key) => Some(key),
            Self::Ime(_) => None,
        }
    }
}

pub(super) enum RawSample {
    Key(KeyboardInput),
    Ime(Ime),
}

/// Captures modifiers at a key edge so later releases cannot change their meaning.
pub(super) fn modifiers(keys: &[String]) -> FieldModifiers {
    let held = |left: &str, right: &str| keys.iter().any(|key| key == left || key == right);
    FieldModifiers {
        shift: held("ShiftLeft", "ShiftRight"),
        control: held("ControlLeft", "ControlRight"),
        alt: held("AltLeft", "AltRight"),
        super_key: held("SuperLeft", "SuperRight"),
    }
}

/// Uses the platform/developer-control stream's original order without replaying typed mirrors.
/// Adapters without that stream apply pending IME commits before their keyboard samples.
pub(super) fn samples(
    physical: &mut PhysicalControls,
    ordered: Option<&Messages<WindowEvent>>,
    keyboard: Option<&Messages<KeyboardInput>>,
    ime: Option<&Messages<Ime>>,
    window: Entity,
) -> Vec<RawSample> {
    if let Some(events) = ordered {
        let samples: Vec<RawSample> = physical
            .ordered
            .read(events)
            .filter_map(|event| match event {
                WindowEvent::KeyboardInput(event) if event.window == window => {
                    Some(RawSample::Key(event.clone()))
                }
                WindowEvent::Ime(event) if ime_window(event) == window => {
                    Some(RawSample::Ime(event.clone()))
                }
                _ => None,
            })
            .collect();
        if !samples.is_empty() {
            if let Some(events) = keyboard {
                physical.keys.clear(events);
            }
            if let Some(events) = ime {
                physical.ime.clear(events);
            }
            return samples;
        }
    }
    let mut samples = Vec::new();
    if let Some(events) = ime {
        samples.extend(
            physical
                .ime
                .read(events)
                .filter(|event| ime_window(event) == window)
                .cloned()
                .map(RawSample::Ime),
        );
    }
    if let Some(events) = keyboard {
        samples.extend(
            physical
                .keys
                .read(events)
                .filter(|event| event.window == window)
                .cloned()
                .map(RawSample::Key),
        );
    }
    samples
}

/// Associates every platform IME event with its originating window.
fn ime_window(event: &Ime) -> Entity {
    match event {
        Ime::Preedit { window, .. }
        | Ime::Commit { window, .. }
        | Ime::Enabled { window }
        | Ime::Disabled { window } => *window,
    }
}

/// Applies composition at its position in the input stream, before any subsequent keystroke.
pub(super) fn route_sample(
    sample: PanelSample,
    ui: &mut mod_ui::ModUi,
    accepting: bool,
) -> Option<KeySample> {
    let accepting = accepting && ui.mod_panel_field_focused();
    match sample {
        PanelSample::Key(key) if accepting && ui.mod_panel_composing() && key.0 != "Escape" => None,
        PanelSample::Key(key) => Some(key),
        PanelSample::Ime(event) => {
            if accepting {
                match event {
                    Ime::Preedit { value, cursor, .. } => {
                        ui.mod_panel_preedit(&value, cursor.map(|(start, _)| start));
                    }
                    Ime::Commit { value, .. } => {
                        ui.mod_panel_ime_commit(&value);
                    }
                    Ime::Disabled { .. } => {
                        ui.mod_panel_preedit("", None);
                    }
                    Ime::Enabled { .. } => {}
                }
            }
            None
        }
    }
}

/// Enables platform composition for our focused field without publishing unchanged window state.
pub(super) fn sync_ime(
    physical: &mut PhysicalControls,
    window: &mut Mut<Window>,
    ui: &mut mod_ui::ModUi,
    accepting: bool,
) {
    let enabled = accepting && ui.mod_panel_field_focused();
    if (enabled || physical.ime_owned) && window.ime_enabled != enabled {
        window.ime_enabled = enabled;
    }
    if let Some(position) = ui.mod_panel_ime_position() {
        let position = Vec2::from_array(position);
        if window.ime_position != position {
            window.ime_position = position;
        }
    }
    physical.ime_owned = enabled;
    if !enabled {
        ui.mod_panel_preedit("", None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts updates whose window was marked changed, which triggers a native sync.
    #[derive(Default, Resource)]
    struct WindowChanges(u32);

    fn sync_idle_ime(mut windows: Query<&mut Window>) {
        sync_ime(
            &mut PhysicalControls::default(),
            &mut windows.single_mut().unwrap(),
            &mut mod_ui::ModUi::default(),
            false,
        );
    }

    fn count_window_changes(
        changed: Query<(), Changed<Window>>,
        mut changes: ResMut<WindowChanges>,
    ) {
        changes.0 += u32::from(!changed.is_empty());
    }

    /// This runs every frame a mod is active, so it must not publish an unchanged window.
    #[test]
    fn ime_sync_leaves_an_unchanged_window_unmarked() {
        let mut app = App::new();
        app.init_resource::<WindowChanges>()
            .add_systems(Update, (sync_idle_ime, count_window_changes).chain());
        app.world_mut().spawn(Window::default());
        app.update();
        app.world_mut().resource_mut::<WindowChanges>().0 = 0;
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<WindowChanges>().0, 0);
    }

    /// Physical enum names must keep matching every native field modifier, including right keys.
    #[test]
    fn native_field_modifiers_follow_physical_key_names() {
        for (keys, expected) in [
            (
                [KeyCode::ShiftLeft, KeyCode::ShiftRight],
                FieldModifiers {
                    shift: true,
                    ..Default::default()
                },
            ),
            (
                [KeyCode::ControlLeft, KeyCode::ControlRight],
                FieldModifiers {
                    control: true,
                    ..Default::default()
                },
            ),
            (
                [KeyCode::AltLeft, KeyCode::AltRight],
                FieldModifiers {
                    alt: true,
                    ..Default::default()
                },
            ),
            (
                [KeyCode::SuperLeft, KeyCode::SuperRight],
                FieldModifiers {
                    super_key: true,
                    ..Default::default()
                },
            ),
        ] {
            for key in keys {
                assert_eq!(modifiers(&[format!("{key:?}")]), expected);
            }
        }
        assert_eq!(
            modifiers(&[format!("{:?}", KeyCode::KeyA)]),
            FieldModifiers::default()
        );
    }
}
