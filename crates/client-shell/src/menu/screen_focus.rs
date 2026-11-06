//! Publishes screen ownership and menu-configured controls to gameplay.
use bevy::prelude::{ButtonInput, Gamepad, KeyCode, MouseButton, Query, Res, ResMut};
use client_state::{GameplayScreenControls, PlayerRuntime, ScreenFocus};
use semantic_input::InputMode;

use super::{MenuRuntime, settings_options::control_bindings};
use crate::ui_resources::{UiPresentationResource, UiRuntimeResource};
use launcher::menu::settings_options::{BindingId, SettingId};

/// Samples the current screen stack after shell and UI changes become authoritative.
pub fn publish_screen_focus(
    player: Res<PlayerRuntime>,
    ui: Option<Res<UiRuntimeResource>>,
    menu: Option<Res<MenuRuntime>>,
    presentation: Option<Res<UiPresentationResource>>,
    mut focus: ResMut<ScreenFocus>,
) {
    *focus = ScreenFocus {
        menu_visible: menu.as_deref().is_some_and(MenuRuntime::is_visible),
        ui_focused: ui.as_deref().is_some_and(|ui| ui.ui_focused(&player)),
        absorbs_input: crate::screen_policy::absorbs_input(
            &player,
            ui.as_deref().map(|ui| &ui.0),
            menu.as_deref(),
            presentation.as_deref().map(|presentation| &presentation.0),
        ),
        steals_mouse: ui.as_deref().map(|ui| {
            ui.steals_mouse(
                &player,
                menu.as_deref().map(|menu| {
                    menu as &dyn client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene
                }),
            )
        }),
    };
}

/// Samples bindings after cursor capture retires consumed input, before camera look.
pub fn publish_gameplay_screen_controls(
    menu: Option<Res<MenuRuntime>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    pads: Query<&Gamepad>,
    mut presentation: Option<ResMut<UiPresentationResource>>,
    mut controls: ResMut<GameplayScreenControls>,
) {
    if let (Some(menu), Some(ref mut presentation)) = (menu.as_deref(), presentation.as_mut()) {
        if menu.touch_available {
            presentation.enable_touch_controls(menu.gameplay_input_mode == InputMode::Touch);
        }
    }
    *controls = GameplayScreenControls {
        pick_block_pressed: menu.is_some()
            && (control_bindings::binding_gamepad(menu.as_deref(), BindingId::PickItem, &pads)
                || keys.zip(mouse).is_some_and(|(keys, mouse)| {
                    control_bindings::binding_pressed(
                        menu.as_deref(),
                        BindingId::PickItem,
                        &keys,
                        &mouse,
                    )
                })),
        spyglass_mouse_damping: menu
            .as_deref()
            .map_or(0.0, |menu| menu.spyglass_damping(InputMode::KeyboardMouse)),
        spyglass_gamepad_damping: menu
            .as_deref()
            .map_or(0.0, |menu| menu.spyglass_damping(InputMode::GamePad)),
        hide_hud: menu
            .as_deref()
            .is_some_and(|menu| menu.settings_snapshot().0.value(SettingId::HideHud) != 0),
        locked_input_mode: menu
            .as_deref()
            .filter(|menu| menu.touch_available)
            .map(|menu| menu.gameplay_input_mode),
    };
}

impl MenuRuntime {
    /// The currently selected gameplay input mode.
    pub fn gameplay_input_mode(&self) -> InputMode {
        self.gameplay_input_mode
    }

    /// Sets the gameplay input mode, constraining it to `Touch` or `KeyboardMouse`.
    pub fn set_gameplay_input_mode(&mut self, mode: InputMode) {
        self.gameplay_input_mode = match mode {
            InputMode::Touch => InputMode::Touch,
            InputMode::KeyboardMouse | InputMode::GamePad => InputMode::KeyboardMouse,
        };
    }

    /// Toggles between `Touch` and `KeyboardMouse`, never selecting `GamePad`.
    pub fn toggle_gameplay_input_mode(&mut self) -> InputMode {
        self.gameplay_input_mode = match self.gameplay_input_mode {
            InputMode::Touch => InputMode::KeyboardMouse,
            InputMode::KeyboardMouse | InputMode::GamePad => InputMode::Touch,
        };
        self.gameplay_input_mode
    }

    /// Overrides touch availability for tests.
    #[cfg(test)]
    pub fn set_touch_available(&mut self, available: bool) {
        self.touch_available = available;
    }
}

#[cfg(test)]
mod tests;
