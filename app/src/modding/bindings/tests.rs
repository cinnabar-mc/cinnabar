use super::*;
use semantic_input::{Action, InputChord, ModifierChord};

/// Rebinding replaces the exported physical chord immediately, and idle frames reuse it.
#[test]
fn actual_rebinds_and_modifiers_change_the_cached_revision_before_joining() {
    let mut cache = Bindings::default();
    let original = cache.sample(ControlSettings::builtin());
    assert!(
        original
            .entries
            .iter()
            .any(|b| b.action == "attack" && b.key == "MouseLeft")
    );
    assert!(Arc::ptr_eq(
        &original,
        &cache.sample(ControlSettings::builtin())
    ));
    let settings = ControlSettings::new(
        vec![ActionBinding {
            action: Action::Attack,
            context: InputContext::Gameplay,
            chord: InputChord {
                control: PhysicalControl::KeyboardUsage(0x0e),
                modifiers: ModifierChord {
                    shift: true,
                    ..Default::default()
                },
            },
        }],
        0.5,
        1.,
        1.,
        false,
        false,
        0.15,
        0.15,
    )
    .unwrap();
    let mut runtime = client_state::RuntimeSettings::default();
    let mut user = runtime.user_settings_update().1.clone();
    user.controls = settings;
    runtime.replace_user_settings(user);
    let rebound = cache.sample(runtime.controls_update().1);
    assert_ne!(rebound.revision, original.revision);
    assert_eq!(rebound.entries.len(), 1);
    assert_eq!(rebound.entries[0].key, "KeyK");
    assert!(rebound.entries[0].modifiers.shift);
}

/// Every exported keyboard name uses the exact Bevy name read by physical controls.
#[test]
fn key_names_match_physical_observations_and_devices_stay_explicit() {
    for (code, key) in client_shell::physical_input::DESKTOP_KEYS {
        let binding = ActionBinding {
            action: Action::Jump,
            context: InputContext::Gameplay,
            chord: InputChord {
                control: PhysicalControl::KeyboardUsage(key.usage().unwrap()),
                modifiers: ModifierChord::default(),
            },
        };
        assert_eq!(project(&binding).unwrap().key, format!("{code:?}"));
    }
    for (button, name) in super::super::input::POINTER_BUTTONS {
        let binding = ActionBinding {
            action: Action::Attack,
            context: InputContext::Gameplay,
            chord: InputChord {
                control: PhysicalControl::MouseButton(
                    client_shell::physical_input::mouse_button_code(button).unwrap(),
                ),
                modifiers: ModifierChord::default(),
            },
        };
        assert_eq!(project(&binding).unwrap().key, name);
    }
    for control in [
        PhysicalControl::GamepadButton(0),
        PhysicalControl::TouchControl(0),
        PhysicalControl::MouseButton(4),
    ] {
        let binding = ActionBinding {
            action: Action::Attack,
            context: InputContext::Gameplay,
            chord: InputChord {
                control,
                modifiers: ModifierChord::default(),
            },
        };
        assert!(project(&binding).is_none());
    }
}
