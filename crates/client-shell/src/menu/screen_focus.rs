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
    effective_mode: Option<Res<client_state::EffectiveGameplayInputMode>>,
    mut presentation: Option<ResMut<UiPresentationResource>>,
    mut controls: ResMut<GameplayScreenControls>,
) {
    if let (Some(menu), Some(presentation)) = (menu.as_deref(), presentation.as_mut()) {
        sync_touch_controls(menu, effective_mode.map(|mode| mode.0), presentation);
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
    };
}

/// Shows the touch controls only where touch is available and, when the platform arbitrates a
/// gameplay input mode, only in `Touch` mode. Without a mode the current state is kept.
///
/// Writes only on change, keeping presentation change detection and open touch utilities intact
/// while the mode holds.
pub fn sync_touch_controls(
    menu: &MenuRuntime,
    mode: Option<InputMode>,
    presentation: &mut ResMut<UiPresentationResource>,
) {
    let enabled = if !menu.touch_available {
        Some(false)
    } else {
        mode.map(|mode| mode == InputMode::Touch)
    };
    if let Some(enabled) = enabled
        && presentation.touch_controls_enabled() != enabled
    {
        presentation.enable_touch_controls(enabled);
    }
}

impl MenuRuntime {
    /// Overrides touch availability for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_touch_available(&mut self, available: bool) {
        self.touch_available = available;
    }
}

#[cfg(test)]
mod tests;
