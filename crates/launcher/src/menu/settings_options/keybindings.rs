//! Desktop key remapping uses the same physical controls as the gameplay router.

use super::{
    BindingId, SettingsOptions,
    bindings::{EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, KEY_BINDINGS, LATE_DEFAULTS},
    control_bindings::{
        GAMEPAD_OFFSET, UNBOUND_CONTROL, decode_control, encode_control, is_gamepad,
    },
};
use semantic_input::{
    Action, ActionBinding, ControlSettings, InputChord, InputContext, PhysicalControl,
};

pub const DEBUG_OVERLAY_KEY: &str = BindingId::DebugOverlay.name();
pub const PLAYER_LIST_KEY: &str = BindingId::PlayerList.name();

/// Vanilla's "Open Notification" binding name.
pub const OPEN_NOTIFICATION_KEY: &str = BindingId::InteractWithToast.name();

/// Labels for client controls that have no installed language-pack entry.
pub fn custom_key_label(name: &str) -> Option<&'static str> {
    BindingId::from_name(name)?.definition().label
}

impl SettingsOptions {
    /// Captures active yielding defaults before a remap can change their conflicts.
    fn active_late_defaults(&self) -> [(Action, Option<PhysicalControl>); LATE_DEFAULTS.len()] {
        LATE_DEFAULTS.map(|action| {
            let index = KEY_BINDINGS
                .iter()
                .position(|(candidate, _)| *candidate == action)
                .expect("late defaults belong to the keyboard layout");
            (action, self.key_control(index))
        })
    }

    /// Legacy layouts can suppress new defaults; interactive edits retain active actions.
    fn preserve_late_defaults(
        &mut self,
        previous: [(Action, Option<PhysicalControl>); LATE_DEFAULTS.len()],
    ) -> [Option<&'static str>; LATE_DEFAULTS.len()] {
        let mut inserted = [None; LATE_DEFAULTS.len()];
        for (index, (action, control)) in previous.into_iter().enumerate() {
            if self.default_yields(action)
                && let Some(control) = control
            {
                let (_, name) = KEY_BINDINGS
                    .iter()
                    .find(|(candidate, _)| *candidate == action)
                    .expect("late defaults belong to the keyboard layout");
                let code = encode_control(control).expect("resolved controls have persisted codes");
                self.keys.insert((*name).to_owned(), code);
                inserted[index] = Some(*name);
            }
        }
        inserted
    }

    /// Lets an untouched late default yield to a conflicting saved keyboard control.
    fn default_yields(&self, action: Action) -> bool {
        let Some((_, name)) = KEY_BINDINGS
            .iter()
            .find(|(candidate, _)| *candidate == action)
        else {
            return false;
        };
        if !LATE_DEFAULTS.contains(&action) || self.keys.contains_key(*name) {
            return false;
        }
        let defaults = ControlSettings::builtin();
        defaults
            .bindings()
            .iter()
            .find(|binding| binding.action == action)
            .is_some_and(|binding| self.saved_keyboard_control(binding.chord.control))
    }

    /// Whether a saved keyboard and mouse row, other than a command macro, holds `control`.
    fn saved_keyboard_control(&self, control: PhysicalControl) -> bool {
        self.keys.iter().any(|(name, code)| {
            !name.starts_with("gamepad:")
                && !name.starts_with(super::command_macros::MACRO_KEY_PREFIX)
                && decode_control(*code) == Some(control)
        })
    }
    /// Validates stored controls with the same device rules as interactive remapping.
    pub fn stored_bindings_valid(&self) -> bool {
        self.controls().is_ok()
            && (0..KEY_BINDINGS.len() + EXTRA_KEYS.len())
                .chain(
                    GAMEPAD_OFFSET..GAMEPAD_OFFSET + GAMEPAD_BINDINGS.len() + EXTRA_GAMEPAD.len(),
                )
                .all(|index| {
                    let Some((name, _, _)) = self.binding(index) else {
                        return true;
                    };
                    let Some(code) = self.keys.get(name) else {
                        return true;
                    };
                    *code == UNBOUND_CONTROL
                        || decode_control(*code)
                            .is_some_and(|control| is_gamepad(control) == (index >= GAMEPAD_OFFSET))
                })
    }

    /// Resolves a settings row to its persisted name and optional semantic action.
    fn binding(
        &self,
        index: usize,
    ) -> Option<(&'static str, Option<Action>, Option<PhysicalControl>)> {
        if let Some(index) = index.checked_sub(GAMEPAD_OFFSET) {
            let name = gamepad_row_names().get(index)?.as_str();
            if let Some((action, _)) = GAMEPAD_BINDINGS.get(index) {
                return Some((name, Some(*action), None));
            }
            let (row, control) = EXTRA_GAMEPAD.get(index.checked_sub(GAMEPAD_BINDINGS.len())?)?;
            return Some((
                name,
                BindingId::from_name(row)?.definition().action,
                *control,
            ));
        }
        if let Some((action, name)) = KEY_BINDINGS.get(index) {
            return Some((name, Some(*action), None));
        }
        let (name, control) = EXTRA_KEYS.get(index.checked_sub(KEY_BINDINGS.len())?)?;
        Some((
            name,
            BindingId::from_name(name)?.definition().action,
            Some(*control),
        ))
    }

    /// Reads the persisted device binding, falling back to the gameplay router's defaults.
    pub fn key_control(&self, index: usize) -> Option<PhysicalControl> {
        let (name, action, fallback) = self.binding(index)?;
        if self.keys.get(name) == Some(&UNBOUND_CONTROL) {
            return None;
        }
        if action.is_some_and(|action| self.default_yields(action)) {
            return None;
        }
        // Layouts saved before Menu Cancel existed can hold its default on another action.
        if name == BindingId::MenuCancel.name()
            && !self.keys.contains_key(name)
            && fallback.is_some_and(|default| self.saved_keyboard_control(default))
        {
            return None;
        }
        self.keys
            .get(name)
            .and_then(|code| decode_control(*code))
            .filter(|control| is_gamepad(*control) == (index >= GAMEPAD_OFFSET))
            .or(fallback)
            .or_else(|| {
                ControlSettings::builtin()
                    .bindings()
                    .iter()
                    .find(|binding| {
                        binding.context == InputContext::Gameplay
                            && Some(binding.action) == action
                            && is_gamepad(binding.chord.control) == (index >= GAMEPAD_OFFSET)
                            && !matches!(binding.chord.control, PhysicalControl::MouseAxis(_))
                    })
                    .map(|binding| binding.chord.control)
            })
            .map(|control| self.swap_gamepad_control(control))
    }

    /// Reads a registered UI action through the settings grid's remapping table.
    ///
    /// ```compile_fail
    /// let options = launcher::menu::settings_options::SettingsOptions::default();
    /// options.named_key_control("key.inventory");
    /// ```
    pub fn named_key_control(&self, id: BindingId) -> Option<PhysicalControl> {
        self.key_control(id.row(false)?)
    }

    /// Keeps secondary defaults until an explicit remap replaces the key list.
    pub fn secondary_key_control(&self, id: BindingId) -> Option<PhysicalControl> {
        (!self.keys.contains_key(id.name()))
            .then_some(id.definition().secondary)
            .flatten()
    }

    /// Restores one device's full layout without disturbing bindings on the other device.
    ///
    /// Macro rows are keyboard and mouse only: the keyboard reset clears them
    /// while the gamepad reset keeps them.
    pub fn reset_bindings(&mut self, gamepad: bool) {
        self.keys
            .retain(|name, _| name.starts_with("gamepad:") != gamepad);
    }

    /// Saves a mapping only if the complete gameplay binding set remains valid. A control taken
    /// from Menu Cancel leaves that row unassigned, so a screen no longer cancels on it.
    pub fn remap(&mut self, index: usize, control: PhysicalControl) -> bool {
        let Some((name, _, _)) = self.binding(index) else {
            return false;
        };
        if is_gamepad(control) != (index >= GAMEPAD_OFFSET) {
            return false;
        }
        let Some(code) = encode_control(self.swap_gamepad_control(control)) else {
            return false;
        };
        let takes_menu_cancel = name != BindingId::MenuCancel.name()
            && index < GAMEPAD_OFFSET
            && self.named_key_control(BindingId::MenuCancel) == Some(control);
        let active_defaults = self.active_late_defaults();
        let previous = self.keys.insert(name.to_owned(), code);
        let inserted_defaults = self.preserve_late_defaults(active_defaults);
        if self.controls().is_err() {
            for name in inserted_defaults.into_iter().flatten() {
                self.keys.remove(name);
            }
            match previous {
                Some(code) => {
                    self.keys.insert(name.to_owned(), code);
                }
                None => {
                    self.keys.remove(name);
                }
            }
            return false;
        }
        if takes_menu_cancel {
            self.keys
                .insert(BindingId::MenuCancel.name().to_owned(), UNBOUND_CONTROL);
        }
        true
    }

    /// Clears one action's keyboard and mouse assignments, preserving the gamepad layout.
    pub fn unbind_key(&mut self, index: usize) -> bool {
        if index >= GAMEPAD_OFFSET {
            return false;
        }
        let Some((name, _, _)) = self.binding(index) else {
            return false;
        };
        let previous_defaults = self.active_late_defaults();
        self.keys.insert(name.to_owned(), UNBOUND_CONTROL);
        for (action, control) in previous_defaults {
            if control.is_none() {
                let (_, name) = KEY_BINDINGS
                    .iter()
                    .find(|(candidate, _)| *candidate == action)
                    .expect("late defaults belong to the keyboard layout");
                self.keys
                    .entry((*name).to_owned())
                    .or_insert(UNBOUND_CONTROL);
            }
        }
        true
    }

    /// Restores one action's default control while preserving other remaps.
    pub fn reset_key(&mut self, index: usize) -> bool {
        let Some((name, action, fallback)) = self.binding(index) else {
            return false;
        };
        let mut active_defaults = self.active_late_defaults();
        if let Some((_, control)) = active_defaults
            .iter_mut()
            .find(|(candidate, _)| Some(*candidate) == action)
        {
            *control = Self::default().key_control(index);
        }
        let previous = self.keys.remove(name);
        let displaced = if name == BindingId::MenuCancel.name() {
            self.unassign_saved_keyboard_control(fallback)
        } else {
            Vec::new()
        };
        let inserted_defaults = self.preserve_late_defaults(active_defaults);
        if self.controls().is_err() {
            for name in inserted_defaults.into_iter().flatten() {
                self.keys.remove(name);
            }
            self.keys.extend(displaced);
            if let Some(previous) = previous {
                self.keys.insert(name.to_owned(), previous);
            }
            return false;
        }
        true
    }

    /// Unassigns the saved rows holding `control`, as vanilla's row reset does for its restored
    /// key, and returns their previous entries.
    fn unassign_saved_keyboard_control(
        &mut self,
        control: Option<PhysicalControl>,
    ) -> Vec<(String, u16)> {
        let displaced: Vec<_> = self
            .keys
            .iter()
            .filter(|(name, code)| {
                !name.starts_with("gamepad:")
                    && !name.starts_with(super::command_macros::MACRO_KEY_PREFIX)
                    && control.is_some()
                    && decode_control(**code) == control
            })
            .map(|(name, code)| (name.clone(), *code))
            .collect();
        for (name, _) in &displaced {
            self.keys.insert(name.clone(), UNBOUND_CONTROL);
        }
        displaced
    }

    /// Rebuilds and validates the gameplay bindings before handing them to the router.
    pub fn controls(&self) -> Result<ControlSettings, semantic_input::BindingError> {
        let original = ControlSettings::builtin();
        let mut bindings = original.bindings().to_vec();
        for action in LATE_DEFAULTS {
            if self.default_yields(action) {
                bindings.retain(|binding| binding.action != action);
            }
        }
        for index in (0..KEY_BINDINGS.len() + EXTRA_KEYS.len()).chain(
            (0..GAMEPAD_BINDINGS.len() + EXTRA_GAMEPAD.len()).map(|index| GAMEPAD_OFFSET + index),
        ) {
            let Some((name, Some(action), _)) = self.binding(index) else {
                continue;
            };
            let Some(code) = self.keys.get(name) else {
                continue;
            };
            let control = decode_control(*code);
            if control.is_none() && *code != UNBOUND_CONTROL {
                continue;
            }
            let mut replaced = false;
            bindings.retain_mut(|binding| {
                if binding.action != action
                    || binding.context != InputContext::Gameplay
                    || is_gamepad(binding.chord.control) != (index >= GAMEPAD_OFFSET)
                    || matches!(
                        binding.chord.control,
                        PhysicalControl::MouseAxis(_) | PhysicalControl::TouchControl(_)
                    )
                {
                    return true;
                }
                let Some(control) = control else {
                    return false;
                };
                if replaced {
                    return false;
                }
                binding.chord.control = control;
                replaced = true;
                true
            });
            if !replaced && let Some(control) = control {
                bindings.push(ActionBinding {
                    action,
                    context: InputContext::Gameplay,
                    chord: InputChord {
                        control,
                        modifiers: Default::default(),
                    },
                });
            }
        }
        for binding in &mut bindings {
            binding.chord.control = self.swap_gamepad_control(binding.chord.control);
        }
        ControlSettings::new(
            bindings,
            original.mouse_sensitivity,
            original.gamepad_look_sensitivity,
            original.touch_look_sensitivity,
            original.invert_mouse_y,
            original.invert_gamepad_y,
            original.gamepad_move_deadzone,
            original.gamepad_look_deadzone,
        )
    }
}

/// Persisted names of the gamepad rows, `gamepad:` before each row's name, in row order.
fn gamepad_row_names() -> &'static [String] {
    static NAMES: std::sync::LazyLock<Box<[String]>> = std::sync::LazyLock::new(|| {
        GAMEPAD_BINDINGS
            .iter()
            .map(|(_, name)| *name)
            .chain(EXTRA_GAMEPAD.iter().map(|(name, _)| *name))
            .map(|name| format!("gamepad:{name}"))
            .collect()
    });
    &NAMES
}

impl SettingsOptions {
    /// Uses the most recently observed layout label, falling back to the physical name.
    pub fn key_name(&self, control: PhysicalControl) -> String {
        if let PhysicalControl::KeyboardUsage(usage) = control
            && let Some(label) = self.key_labels.get(&usage)
        {
            return label.clone();
        }
        key_name(control)
    }

    /// Records a printable layout character without changing any saved binding.
    pub fn set_key_label(&mut self, usage: u16, text: &str) {
        if !matches!(usage, 0x04..=0x27 | 0x2d..=0x38 | 0x64) {
            return;
        }
        let mut chars = text.chars();
        let Some(character) = chars.next() else {
            return;
        };
        if chars.next().is_some() || character.is_control() || character.is_whitespace() {
            return;
        }
        self.key_labels.insert(usage, text.to_uppercase());
    }
}

/// Displays a physical keyboard, mouse or gamepad control without a layout label.
pub fn key_name(control: PhysicalControl) -> String {
    match control {
        PhysicalControl::KeyboardUsage(code @ 0x04..=0x1d) => {
            char::from(b'A' + (code - 4) as u8).to_string()
        }
        PhysicalControl::KeyboardUsage(code @ 0x1e..=0x26) => (code - 0x1d).to_string(),
        PhysicalControl::KeyboardUsage(0x27) => "0".to_owned(),
        PhysicalControl::KeyboardUsage(0x2b) => "Tab".to_owned(),
        PhysicalControl::KeyboardUsage(0x2c) => "Space".to_owned(),
        PhysicalControl::KeyboardUsage(0xe0) => "Left Control".to_owned(),
        PhysicalControl::KeyboardUsage(0xe1) => "Left Shift".to_owned(),
        PhysicalControl::KeyboardUsage(code @ 0x3a..=0x45) => format!("F{}", code - 0x39),
        PhysicalControl::KeyboardUsage(0x39) => "Caps Lock".to_owned(),
        PhysicalControl::KeyboardUsage(0x46) => "Print Screen".to_owned(),
        PhysicalControl::KeyboardUsage(0x47) => "Scroll Lock".to_owned(),
        PhysicalControl::KeyboardUsage(0x48) => "Pause".to_owned(),
        PhysicalControl::KeyboardUsage(0x53) => "Num Lock".to_owned(),
        PhysicalControl::KeyboardUsage(0x54) => "Numpad /".to_owned(),
        PhysicalControl::KeyboardUsage(0x55) => "Numpad *".to_owned(),
        PhysicalControl::KeyboardUsage(0x56) => "Numpad -".to_owned(),
        PhysicalControl::KeyboardUsage(0x57) => "Numpad +".to_owned(),
        PhysicalControl::KeyboardUsage(0x58) => "Numpad Enter".to_owned(),
        PhysicalControl::KeyboardUsage(0x62) => "Numpad 0".to_owned(),
        PhysicalControl::KeyboardUsage(0x63) => "Numpad .".to_owned(),
        PhysicalControl::KeyboardUsage(0x64) => "Intl Backslash".to_owned(),
        PhysicalControl::KeyboardUsage(0x65) => "Context Menu".to_owned(),
        PhysicalControl::KeyboardUsage(0x67) => "Numpad =".to_owned(),
        PhysicalControl::KeyboardUsage(0x85) => "Numpad ,".to_owned(),
        PhysicalControl::KeyboardUsage(code @ 0x59..=0x61) => format!("Numpad {}", code - 0x58),
        PhysicalControl::KeyboardUsage(code @ 0x68..=0x73) => format!("F{}", code - 0x5b),
        PhysicalControl::MouseButton(button) => format!("Mouse {button}"),
        PhysicalControl::KeyboardUsage(code) => format!("Key {code:02X}"),
        PhysicalControl::GamepadButton(0) => "A".to_owned(),
        PhysicalControl::GamepadButton(1) => "B".to_owned(),
        PhysicalControl::GamepadButton(2) => "Y".to_owned(),
        PhysicalControl::GamepadButton(3) => "X".to_owned(),
        PhysicalControl::GamepadButton(4) => "LB".to_owned(),
        PhysicalControl::GamepadButton(5) => "RB".to_owned(),
        PhysicalControl::GamepadButton(8) => "LS".to_owned(),
        PhysicalControl::GamepadButton(9) => "RS".to_owned(),
        PhysicalControl::GamepadButton(11) => "D-pad Up".to_owned(),
        PhysicalControl::GamepadButton(12) => "D-pad Down".to_owned(),
        PhysicalControl::GamepadButton(13) => "D-pad Left".to_owned(),
        PhysicalControl::GamepadButton(14) => "D-pad Right".to_owned(),
        PhysicalControl::GamepadAxis { axis: 4, .. } => "LT".to_owned(),
        PhysicalControl::GamepadAxis { axis: 5, .. } => "RT".to_owned(),
        _ => String::new(),
    }
}

#[cfg(test)]
#[path = "overlay_binding_tests.rs"]
mod overlay_binding_tests;

#[cfg(test)]
#[path = "unbound_tests.rs"]
mod unbound_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_defaults_and_row_names_match_their_sources() {
        assert_eq!(ControlSettings::builtin(), &ControlSettings::default());
        for (index, (_, name)) in GAMEPAD_BINDINGS.iter().enumerate() {
            assert_eq!(gamepad_row_names()[index], format!("gamepad:{name}"));
        }
        for (index, (name, _)) in EXTRA_GAMEPAD.iter().enumerate() {
            assert_eq!(
                gamepad_row_names()[GAMEPAD_BINDINGS.len() + index],
                format!("gamepad:{name}")
            );
        }
        assert_eq!(
            gamepad_row_names().len(),
            GAMEPAD_BINDINGS.len() + EXTRA_GAMEPAD.len()
        );
    }

    /// Bindings resolve from the saved settings on every read, so an edit applies the next frame.
    #[test]
    fn remaps_and_unbinds_resolve_on_the_next_read() {
        let mut settings = SettingsOptions::default();
        let pad = BindingId::PickItem.row(true).unwrap();
        let key = BindingId::PickItem.row(false).unwrap();
        assert!(settings.remap(pad, PhysicalControl::GamepadButton(9)));
        assert_eq!(
            settings.key_control(pad),
            Some(PhysicalControl::GamepadButton(9))
        );
        assert!(settings.remap(key, PhysicalControl::KeyboardUsage(0x15)));
        assert_eq!(
            settings.named_key_control(BindingId::PickItem),
            Some(PhysicalControl::KeyboardUsage(0x15))
        );
        assert!(settings.unbind_key(key));
        assert_eq!(settings.named_key_control(BindingId::PickItem), None);
        assert!(settings.reset_key(key));
        assert_eq!(
            settings.named_key_control(BindingId::PickItem),
            SettingsOptions::default().named_key_control(BindingId::PickItem)
        );
    }

    /// Taking Menu Cancel's control unassigns it, and it stays unassigned after the action moves on.
    #[test]
    fn remapping_an_action_onto_menu_cancel_unassigns_it() {
        let mut settings = SettingsOptions::default();
        let hotbar = BindingId::Hotbar4.row(false).unwrap();
        let back = settings.named_key_control(BindingId::MenuCancel).unwrap();
        assert!(settings.remap(hotbar, back));
        assert_eq!(settings.key_control(hotbar), Some(back));
        assert_eq!(settings.named_key_control(BindingId::MenuCancel), None);
        assert!(settings.remap(hotbar, PhysicalControl::KeyboardUsage(0x15)));
        assert_eq!(settings.named_key_control(BindingId::MenuCancel), None);
    }

    /// Menu Cancel's own control and other buttons leave the row bound.
    #[test]
    fn other_remaps_keep_menu_cancel() {
        let mut settings = SettingsOptions::default();
        let back = settings.named_key_control(BindingId::MenuCancel).unwrap();
        let hotbar = BindingId::Hotbar4.row(false).unwrap();
        assert!(settings.remap(hotbar, PhysicalControl::KeyboardUsage(0x15)));
        assert_eq!(
            settings.named_key_control(BindingId::MenuCancel),
            Some(back)
        );
        let cancel = BindingId::MenuCancel.row(false).unwrap();
        assert!(settings.remap(cancel, back));
        assert_eq!(
            settings.named_key_control(BindingId::MenuCancel),
            Some(back)
        );
    }

    /// A layout saved before Menu Cancel existed keeps its action on the button.
    #[test]
    fn saved_action_on_menu_cancels_default_keeps_the_button() {
        let back = SettingsOptions::default()
            .named_key_control(BindingId::MenuCancel)
            .unwrap();
        let mut settings = SettingsOptions::default();
        settings.keys.insert(
            BindingId::Hotbar4.name().to_owned(),
            encode_control(back).unwrap(),
        );
        assert_eq!(settings.named_key_control(BindingId::MenuCancel), None);
        assert_eq!(settings.named_key_control(BindingId::Hotbar4), Some(back));
    }

    /// Resetting Menu Cancel restores its button over the action holding it, across a reload.
    #[test]
    fn resetting_menu_cancel_takes_its_default_back_from_another_action() {
        let mut settings = SettingsOptions::default();
        let hotbar = BindingId::Hotbar4.row(false).unwrap();
        let cancel = BindingId::MenuCancel.row(false).unwrap();
        let back = settings.named_key_control(BindingId::MenuCancel).unwrap();
        assert!(settings.remap(hotbar, back));
        assert!(settings.reset_key(cancel));
        assert_eq!(
            settings.named_key_control(BindingId::MenuCancel),
            Some(back)
        );
        assert_eq!(settings.key_control(hotbar), None);
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(loaded.named_key_control(BindingId::MenuCancel), Some(back));
        assert_eq!(loaded.key_control(hotbar), None);
        assert!(loaded.stored_bindings_valid());
    }

    /// Resetting another action leaves a Menu Cancel the player already gave up unassigned.
    #[test]
    fn resetting_an_action_keeps_menu_cancel_unassigned() {
        let mut settings = SettingsOptions::default();
        let hotbar = BindingId::Hotbar4.row(false).unwrap();
        let back = settings.named_key_control(BindingId::MenuCancel).unwrap();
        assert!(settings.remap(hotbar, back));
        assert!(settings.reset_key(hotbar));
        assert_eq!(settings.named_key_control(BindingId::MenuCancel), None);
    }

    #[test]
    fn controller_pick_remap_creates_a_semantic_binding() {
        let mut settings = SettingsOptions::default();
        let row = BindingId::PickItem.row(true).unwrap();
        for button in [9, 14] {
            assert!(settings.remap(row, PhysicalControl::GamepadButton(button)));
            let controls = settings.controls().unwrap();
            let picks: Vec<_> = controls
                .bindings()
                .iter()
                .filter(|binding| {
                    binding.context == InputContext::Gameplay
                        && binding.action == Action::PickItem
                        && is_gamepad(binding.chord.control)
                })
                .collect();
            assert_eq!(picks.len(), 1);
            assert_eq!(
                picks[0].chord.control,
                PhysicalControl::GamepadButton(button)
            );
            assert!(controls.bindings().iter().any(|binding| {
                binding.action == Action::PickItem
                    && binding.chord.control == PhysicalControl::MouseButton(3)
            }));
            let mut router = semantic_input::SemanticInputRouter::default();
            router.replace_bindings(controls).unwrap();
            for (index, pressed) in [false, true, true, false, true].into_iter().enumerate() {
                let activity_sequence = index as u64 + 1;
                router
                    .route(semantic_input::DeviceFrame {
                        keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                            activity_sequence: 1,
                            ..Default::default()
                        }),
                        desktop_events: Some(Default::default()),
                        controllers: vec![semantic_input::ControllerFrame {
                            device_id: 1,
                            activity_sequence,
                            buttons: if pressed { vec![button] } else { vec![] },
                            ..Default::default()
                        }],
                        ..Default::default()
                    })
                    .unwrap();
                let snapshot = router.finalize().unwrap();
                let phase = snapshot.phases[Action::PickItem as usize];
                assert_eq!(phase.pressed, pressed && index != 2);
                assert!(!phase.held);
                if pressed {
                    assert_eq!(snapshot.input_mode, semantic_input::InputMode::GamePad);
                    assert!(snapshot.ordered.is_none());
                }
            }
        }
        assert!(settings.reset_key(row));
        assert!(
            !settings
                .controls()
                .unwrap()
                .bindings()
                .iter()
                .any(|binding| {
                    binding.action == Action::PickItem && is_gamepad(binding.chord.control)
                })
        );
    }

    #[test]
    fn fake_azerty_layout_labels_preserve_saved_bindings() {
        let mut settings = SettingsOptions::default();
        let row = KEY_BINDINGS
            .iter()
            .position(|(action, _)| *action == semantic_input::Action::Hotbar1)
            .unwrap();
        assert!(settings.remap(row, PhysicalControl::KeyboardUsage(0x14)));
        let saved = serde_json::to_value(&settings).unwrap();
        for (usage, character, expected) in [
            (0x14, "a", "A"),
            (0x04, "q", "Q"),
            (0x1a, "z", "Z"),
            (0x1d, "w", "W"),
            (0x1f, "é", "É"),
            (0x1e, "&", "&"),
            (0x33, "m", "M"),
        ] {
            settings.set_key_label(usage, character);
            assert_eq!(
                settings.key_name(PhysicalControl::KeyboardUsage(usage)),
                expected
            );
        }
        assert_eq!(serde_json::to_value(&settings).unwrap(), saved);
        let loaded: SettingsOptions = serde_json::from_value(saved).unwrap();
        assert_eq!(loaded.key_control(row), settings.key_control(row));
        assert_eq!(loaded.controls().unwrap(), settings.controls().unwrap());
        assert_eq!(loaded.key_name(PhysicalControl::KeyboardUsage(0x14)), "Q");
    }

    #[test]
    fn layout_labels_update_and_leave_named_controls_unchanged() {
        let mut settings = SettingsOptions::default();
        let q = PhysicalControl::KeyboardUsage(0x14);
        assert_eq!(settings.key_name(q), "Q");
        settings.set_key_label(0x14, "a");
        assert_eq!(settings.key_name(q), "A");
        settings.set_key_label(0x14, "q");
        assert_eq!(settings.key_name(q), "Q");
        for text in ["", "\n", " ", "ab"] {
            settings.set_key_label(0x14, text);
            assert_eq!(settings.key_name(q), "Q");
        }
        for (usage, text, expected) in [
            (0x2c, " ", "Space"),
            (0x59, "1", "Numpad 1"),
            (0xe1, "a", "Left Shift"),
        ] {
            settings.set_key_label(usage, text);
            assert_eq!(
                settings.key_name(PhysicalControl::KeyboardUsage(usage)),
                expected
            );
        }
        assert_eq!(
            settings.key_name(PhysicalControl::MouseButton(1)),
            "Mouse 1"
        );
        assert_eq!(
            settings.key_name(PhysicalControl::KeyboardUsage(0xff)),
            "Key FF"
        );
    }

    #[test]
    fn additional_keyboard_usages_have_readable_names() {
        for (usage, expected) in [
            (0x2b, "Tab"),
            (0x39, "Caps Lock"),
            (0x46, "Print Screen"),
            (0x47, "Scroll Lock"),
            (0x48, "Pause"),
            (0x53, "Num Lock"),
            (0x54, "Numpad /"),
            (0x55, "Numpad *"),
            (0x56, "Numpad -"),
            (0x57, "Numpad +"),
            (0x58, "Numpad Enter"),
            (0x59, "Numpad 1"),
            (0x5a, "Numpad 2"),
            (0x5b, "Numpad 3"),
            (0x5c, "Numpad 4"),
            (0x5d, "Numpad 5"),
            (0x5e, "Numpad 6"),
            (0x5f, "Numpad 7"),
            (0x60, "Numpad 8"),
            (0x61, "Numpad 9"),
            (0x62, "Numpad 0"),
            (0x63, "Numpad ."),
            (0x64, "Intl Backslash"),
            (0x65, "Context Menu"),
            (0x67, "Numpad ="),
            (0x68, "F13"),
            (0x69, "F14"),
            (0x6a, "F15"),
            (0x6b, "F16"),
            (0x6c, "F17"),
            (0x6d, "F18"),
            (0x6e, "F19"),
            (0x6f, "F20"),
            (0x70, "F21"),
            (0x71, "F22"),
            (0x72, "F23"),
            (0x73, "F24"),
            (0x85, "Numpad ,"),
        ] {
            assert_eq!(key_name(PhysicalControl::KeyboardUsage(usage)), expected);
        }
    }
}
