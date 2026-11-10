use super::*;
use semantic_input::{Action, PhysicalControl};

const KEY_G: PhysicalControl = PhysicalControl::KeyboardUsage(0x0a);

/// Edits by action name follow the Controls rules, persist, and repeat as no-ops.
#[test]
fn action_key_edits_apply_like_controls_and_repeat_as_no_ops() {
    let mut settings = SettingsOptions::default();
    let default = settings.named_key_control(BindingId::Freelook);
    assert_eq!(
        settings.change_action_key(Action::Freelook, KeyBindingChange::Assign(default.unwrap())),
        Ok(false)
    );
    assert_eq!(
        settings.change_action_key(Action::Freelook, KeyBindingChange::Assign(KEY_G)),
        Ok(true)
    );
    assert_eq!(
        settings.change_action_key(Action::Freelook, KeyBindingChange::Assign(KEY_G)),
        Ok(false)
    );
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(loaded.named_key_control(BindingId::Freelook), Some(KEY_G));
    assert_eq!(
        settings.change_action_key(Action::Freelook, KeyBindingChange::Unbind),
        Ok(true)
    );
    assert_eq!(settings.named_key_control(BindingId::Freelook), None);
    assert_eq!(
        settings.change_action_key(Action::Freelook, KeyBindingChange::Unbind),
        Ok(false)
    );
    assert_eq!(
        settings.change_action_key(Action::Freelook, KeyBindingChange::Reset),
        Ok(true)
    );
    assert_eq!(settings.named_key_control(BindingId::Freelook), default);
    assert_eq!(
        settings.change_action_key(Action::Freelook, KeyBindingChange::Reset),
        Ok(false)
    );
}

/// Actions without a keyboard row and gamepad controls are refused without changing keys.
#[test]
fn refused_action_key_edits_leave_bindings_unchanged() {
    let mut settings = SettingsOptions::default();
    let before = serde_json::to_vec(&settings).unwrap();
    assert_eq!(
        settings.change_action_key(Action::UiAccept, KeyBindingChange::Assign(KEY_G)),
        Err(KeyBindingRejected::NoKeyboardRow)
    );
    assert_eq!(
        settings.change_action_key(
            Action::Freelook,
            KeyBindingChange::Assign(PhysicalControl::GamepadButton(2))
        ),
        Err(KeyBindingRejected::Refused)
    );
    assert_eq!(serde_json::to_vec(&settings).unwrap(), before);
}
