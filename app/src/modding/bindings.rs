//! Projects current player chords into the physical names observable by personal mods, and
//! applies the binding changes mods request through the Controls settings.

use bevy::prelude::{ResMut, debug};
use client_shell::menu::MenuRuntime;
use launcher::menu::settings_options::KeyBindingChange;
use mod_host::{
    BindingContext, BindingModifiers, InputActionBinding, InputBindingChange, InputBindingWrite,
    InputBindings,
};
use semantic_input::{Action, ActionBinding, ControlSettings, InputContext, PhysicalControl};
use std::sync::Arc;

const _: () = assert!(semantic_input::MAX_BINDINGS == mod_api::MAX_INPUT_BINDINGS);

#[derive(Default)]
pub(super) struct Bindings {
    source: Option<Vec<ActionBinding>>,
    current: Option<Arc<InputBindings>>,
}

impl Bindings {
    /// Reuses the immutable projection until player chords change; no per-frame string work.
    pub(super) fn sample(&mut self, settings: &ControlSettings) -> Arc<InputBindings> {
        if self.source.as_deref() != Some(settings.bindings()) {
            let entries: Vec<_> = settings.bindings().iter().filter_map(project).collect();
            let revision = self
                .current
                .as_ref()
                .map_or(1, |old| old.revision.wrapping_add(1));
            self.current = Some(Arc::new(InputBindings {
                revision,
                total: entries.len() as u32,
                entries,
            }));
            self.source = Some(settings.bindings().to_vec());
        }
        Arc::clone(self.current.as_ref().expect("sampled settings"))
    }
}

/// Keeps only controls that the existing physical key observation can actually name.
fn project(binding: &ActionBinding) -> Option<InputActionBinding> {
    let key = physical_name(binding.chord.control)?;
    let modifiers = binding.chord.modifiers;
    Some(InputActionBinding {
        action: binding.action.binding_name().to_owned(),
        context: match binding.context {
            InputContext::Gameplay => BindingContext::Gameplay,
            InputContext::UiFocused => BindingContext::Ui,
        },
        key,
        modifiers: BindingModifiers {
            shift: modifiers.shift,
            control: modifiers.control,
            alt: modifiers.alt,
            super_key: modifiers.super_key,
        },
    })
}

/// Maps routed controls and player chords through the same desktop naming contract.
pub(super) fn physical_name(control: PhysicalControl) -> Option<String> {
    match control {
        PhysicalControl::KeyboardUsage(usage) => {
            let (code, _) = client_shell::physical_input::DESKTOP_KEYS
                .iter()
                .find(|(_, key)| key.usage() == Some(usage))?;
            Some(format!("{code:?}"))
        }
        PhysicalControl::MouseButton(button) => Some(
            super::input::POINTER_BUTTONS
                .iter()
                .find(|(physical, _)| {
                    client_shell::physical_input::mouse_button_code(*physical) == Some(button)
                })?
                .1
                .to_owned(),
        ),
        _ => None,
    }
}

/// Resolves a physical name from the controls naming contract back to its control.
pub(super) fn physical_control(name: &str) -> Option<PhysicalControl> {
    if let Some((_, key)) = client_shell::physical_input::DESKTOP_KEYS
        .iter()
        .find(|(code, _)| format!("{code:?}") == name)
    {
        return key.usage().map(PhysicalControl::KeyboardUsage);
    }
    let (button, _) = super::input::POINTER_BUTTONS
        .iter()
        .find(|(_, button)| *button == name)?;
    client_shell::physical_input::mouse_button_code(*button).map(PhysicalControl::MouseButton)
}

/// Translates a mod's binding request into a Controls row edit for a known action and control.
fn key_change(write: &InputBindingWrite) -> Result<(Action, KeyBindingChange), &'static str> {
    let action = Action::from_binding_name(&write.action).ok_or("unknown action")?;
    let change = match &write.change {
        InputBindingChange::Assign(key) => {
            KeyBindingChange::Assign(physical_control(key).ok_or("unknown physical key")?)
        }
        InputBindingChange::Unbind => KeyBindingChange::Unbind,
        InputBindingChange::Reset => KeyBindingChange::Reset,
    };
    Ok((action, change))
}

/// Applies player binding changes committed by successful mod callbacks, in load order, as
/// the player's own Controls edits. Without settings, such as before the menu exists, the
/// requests are dropped; refused ones leave the bindings unchanged.
pub(super) fn apply_binding_writes(
    extension: Option<ResMut<super::ModRuntime>>,
    mut menu: Option<ResMut<MenuRuntime>>,
) {
    let Some(mut extension) = extension else {
        return;
    };
    for index in 0..extension.hosts.host_count() {
        for write in extension.hosts.host_mut(index).take_input_binding_writes() {
            let Some(menu) = menu.as_deref_mut() else {
                continue;
            };
            let result = key_change(&write).and_then(|(action, change)| {
                menu.change_action_key(action, change)
                    .map_err(|_| "refused by the Controls settings")
            });
            if let Err(error) = result {
                debug!(action = %write.action, error, "mod binding change refused");
            }
        }
    }
}

/// Copies the finalized router's admission for this callback, independently of game actions.
pub(super) fn admitted_keys(
    input: Option<&semantic_input::ActionSnapshot>,
    owned: bool,
) -> Vec<String> {
    if !owned {
        return Vec::new();
    }
    let mut keys: Vec<_> = input
        .into_iter()
        .flat_map(|snapshot| &snapshot.admitted_desktop)
        .filter_map(|control| physical_name(*control))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.truncate(mod_api::MAX_CONTROL_KEYS);
    keys
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod admission_tests;
