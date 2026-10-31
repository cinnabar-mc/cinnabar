//! Projects current player chords into the physical names observable by personal mods.

use mod_host::{BindingContext, BindingModifiers, InputActionBinding, InputBindings};
use semantic_input::{ActionBinding, ControlSettings, InputContext, PhysicalControl};
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
