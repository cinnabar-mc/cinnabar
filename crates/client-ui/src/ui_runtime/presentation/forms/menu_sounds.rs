//! Which sound a press on the current menu's controls plays.
//!
//! JSON-UI controls declare their own. OreUI controls take vanilla's theme sound: the same click
//! for every button, toggle, link and dropdown, while sliders and text fields have none.

use std::sync::LazyLock;

use launcher::menu::MenuAction;
use launcher::menu::settings_options::{SETTINGS_OPTIONS, SettingKind};
use launcher::shader_options::{Action as ShaderAction, Input as ShaderInput};

use super::super::UiPresentationRuntime;
use crate::sound_requests;

/// The sound names one presentation has played, for tests that run beside other presentations.
#[cfg(any(test, feature = "test-support"))]
pub type CapturedMenuSounds = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// Who decides the press sounds of the menu painted last.
pub(super) enum MenuSounds {
    /// JSON-UI: the sound each painted control declares, by action.
    Declared(Vec<(MenuAction, json_ui::ControlSound)>),
    /// OreUI: the theme's click, for the controls that have one.
    OreUi,
}

impl Default for MenuSounds {
    fn default() -> Self {
        Self::Declared(Vec::new())
    }
}

/// The OreUI theme's click as a control sound.
static OREUI_CLICK: LazyLock<json_ui::ControlSound> = LazyLock::new(|| json_ui::ControlSound {
    name: sound_requests::UI_CLICK.to_owned(),
    volume: 1.0,
    pitch: 1.0,
    min_seconds: 0.0,
});

/// Whether pressing an OreUI control clicks. Sliders and text fields stay silent. So do server
/// list sections, whose click belongs to the press that grabs them, not this release action.
fn oreui_control_clicks(action: MenuAction, scale_picker: bool) -> bool {
    let silent = action.text_field().is_some()
        || match action {
            MenuAction::SettingsScale(_) => !scale_picker,
            MenuAction::SettingsOption(index, _) => SETTINGS_OPTIONS
                .get(usize::from(index))
                .is_some_and(|option| matches!(option.kind, SettingKind::Slider)),
            MenuAction::ShaderOptions(ShaderAction::Element {
                input: ShaderInput::Select(_),
                ..
            })
            | MenuAction::ServerList(_) => true,
            _ => false,
        };
    !silent
}

impl UiPresentationRuntime {
    /// The sound the current menu's control for `action` plays when pressed.
    pub fn menu_sound(&self, action: MenuAction) -> Option<&json_ui::ControlSound> {
        match &self.screens.menu_sounds {
            MenuSounds::Declared(sounds) => sounds
                .iter()
                .find(|(candidate, _)| *candidate == action)
                .map(|(_, sound)| sound),
            MenuSounds::OreUi => {
                let scale_picker = self
                    .menu_view
                    .as_ref()
                    .is_some_and(|view| view.settings.scale_picker);
                oreui_control_clicks(action, scale_picker).then(|| &*OREUI_CLICK)
            }
        }
    }

    /// Plays the press sound of the current menu's control for `action`, if it has one.
    pub fn play_menu_press_sound(&self, action: MenuAction) {
        let Some(sound) = self.menu_sound(action) else {
            return;
        };
        #[cfg(any(test, feature = "test-support"))]
        if let Some(capture) = &self.screens.menu_sound_capture {
            capture.lock().unwrap().push(sound.name.clone());
            return;
        }
        sound_requests::ui_control_sound(sound);
    }

    /// Sends this presentation's press sounds to the returned list instead of the audio queue.
    #[cfg(any(test, feature = "test-support"))]
    pub fn capture_menu_press_sounds(&mut self) -> CapturedMenuSounds {
        self.screens
            .menu_sound_capture
            .get_or_insert_with(Default::default)
            .clone()
    }

    /// Whether the current menu's controls follow the OreUI theme's sound timing.
    pub fn menu_sounds_follow_oreui(&self) -> bool {
        matches!(self.screens.menu_sounds, MenuSounds::OreUi)
    }
}
