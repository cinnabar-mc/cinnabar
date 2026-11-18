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
        Ok(true)
    );
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

/// Assigning an action's default primary key still replaces its default alternatives, as a
/// Controls remap does.
#[test]
fn assigning_the_primary_default_drops_default_alternatives() {
    for (id, action) in [
        (BindingId::Sneak, Action::Sneak),
        (BindingId::Sprint, Action::Sprint),
    ] {
        let mut settings = SettingsOptions::default();
        let primary = settings.named_key_control(id).unwrap();
        settings
            .change_action_key(action, KeyBindingChange::Assign(primary))
            .unwrap();
        let keyboard: Vec<_> = settings
            .controls()
            .unwrap()
            .bindings()
            .iter()
            .filter(|binding| {
                binding.action == action
                    && binding.context == semantic_input::InputContext::Gameplay
                    && matches!(
                        binding.chord.control,
                        PhysicalControl::KeyboardUsage(_) | PhysicalControl::MouseButton(_)
                    )
            })
            .map(|binding| binding.chord.control)
            .collect();
        assert_eq!(keyboard, [primary], "{action:?}");
    }
}
