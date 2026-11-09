//! The touch Control mode option, shown and saved in Bedrock's display order.

use semantic_input::TouchControlMode;

use super::{SettingId, SettingsOptions, definitions::SettingChoice};

/// Each saved value's mode, in the order the option shows its choices.
pub const TOUCH_CONTROL_MODES: [TouchControlMode; 3] = [
    TouchControlMode::Touch,
    TouchControlMode::Classic,
    TouchControlMode::Crosshair,
];

/// The option's choices, derived from [`TOUCH_CONTROL_MODES`].
pub const TOUCH_CONTROL_MODE_CHOICES: &[SettingChoice] = &[
    choice(TOUCH_CONTROL_MODES[0]),
    choice(TOUCH_CONTROL_MODES[1]),
    choice(TOUCH_CONTROL_MODES[2]),
];

/// The saved value of Bedrock's default mode.
pub const DEFAULT_TOUCH_CONTROL_MODE: i32 = 0;

const fn choice(mode: TouchControlMode) -> SettingChoice {
    SettingChoice {
        name: match mode {
            TouchControlMode::Touch => "touch_control_mode_touch",
            TouchControlMode::Crosshair => "touch_control_mode_crosshair",
            TouchControlMode::Classic => "touch_control_mode_classic",
        },
        label: match mode {
            TouchControlMode::Touch => "options.controlMode.touchLabel",
            TouchControlMode::Crosshair => "options.controlMode.crosshairLabel",
            TouchControlMode::Classic => "options.controlMode.classicLabel",
        },
    }
}

/// The help text key describing `mode`.
#[must_use]
pub const fn description_key(mode: TouchControlMode) -> &'static str {
    match mode {
        TouchControlMode::Touch => "options.controlMode.touch.description",
        TouchControlMode::Crosshair => "options.controlMode.crosshair.description",
        TouchControlMode::Classic => "options.controlMode.classic.description",
    }
}

impl SettingsOptions {
    /// The selected touch control mode; an unknown saved value keeps the default.
    #[must_use]
    pub fn touch_control_mode(&self) -> TouchControlMode {
        usize::try_from(self.value(SettingId::TouchControlMode))
            .ok()
            .and_then(|index| TOUCH_CONTROL_MODES.get(index).copied())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const _: () = assert!(TOUCH_CONTROL_MODE_CHOICES.len() == TOUCH_CONTROL_MODES.len());

    #[test]
    fn every_mode_has_one_choice_and_bedrock_defaults_to_touch() {
        for (index, mode) in TOUCH_CONTROL_MODES.into_iter().enumerate() {
            let shown = TOUCH_CONTROL_MODE_CHOICES[index];
            assert_eq!(
                (shown.name, shown.label),
                (choice(mode).name, choice(mode).label)
            );
            assert_eq!(
                TOUCH_CONTROL_MODES
                    .iter()
                    .filter(|other| **other == mode)
                    .count(),
                1
            );
        }
        assert_eq!(
            TOUCH_CONTROL_MODES[DEFAULT_TOUCH_CONTROL_MODE as usize],
            TouchControlMode::Touch
        );
        assert_eq!(
            SettingsOptions::default().touch_control_mode(),
            TouchControlMode::Touch
        );
    }

    #[test]
    fn the_selected_mode_survives_a_save() {
        let mut options = SettingsOptions::default();
        assert!(options.set(SettingId::TouchControlMode.index(), 1));
        let restored = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        assert_eq!(restored.touch_control_mode(), TouchControlMode::Classic);
        assert_eq!(
            restored.user_settings().gameplay.touch_control_mode,
            TouchControlMode::Classic
        );
    }
}
