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
    let key = match binding.chord.control {
        PhysicalControl::KeyboardUsage(usage) => {
            let (code, _) = client_shell::physical_input::DESKTOP_KEYS
                .iter()
                .find(|(_, key)| key.usage() == Some(usage))?;
            format!("{code:?}")
        }
        PhysicalControl::MouseButton(button) => super::input::POINTER_BUTTONS
            .iter()
            .find(|(physical, _)| {
                client_shell::physical_input::mouse_button_code(*physical) == Some(button)
            })?
            .1
            .to_owned(),
        _ => return None,
    };
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

#[cfg(test)]
mod tests;
