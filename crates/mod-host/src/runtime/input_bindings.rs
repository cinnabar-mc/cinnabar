//! Bounded physical binding observations, shared across guest frame callbacks.

use super::State;
use crate::{InputBindings, ModHost};
use anyhow::{Result, ensure};
use std::sync::Arc;

impl ModHost {
    /// Admits a complete host settings snapshot (`total == entries.len()`); imports return
    /// bounded pages and never change native input ownership.
    pub fn observe_input_bindings(&mut self, bindings: Arc<InputBindings>) -> Result<()> {
        if self
            .instance
            .store
            .data()
            .controls
            .bindings
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, &bindings))
        {
            return Ok(());
        }
        validate(&bindings)?;
        self.instance.store.data_mut().controls.bindings = Some(bindings);
        Ok(())
    }
}

/// Checks the response bounds before admitting host settings into guest-visible state.
fn validate(bindings: &InputBindings) -> Result<()> {
    ensure!(
        bindings.entries.len() <= mod_api::MAX_INPUT_BINDINGS
            && bindings.total as usize == bindings.entries.len(),
        "too many input bindings"
    );
    ensure!(
        bindings.entries.iter().all(|binding| {
            !binding.action.is_empty()
                && binding.action.len() <= mod_api::MAX_INPUT_ACTION_BYTES
                && binding
                    .action
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && mod_api::control_key_valid(&binding.key)
        }),
        "invalid physical input binding"
    );
    Ok(())
}

/// Charges the controls read allowance before copying one bounded settings page.
pub(super) fn read(
    state: &mut State,
    after: Option<u64>,
    offset: u32,
    limit: u32,
) -> Result<Result<Option<InputBindings>, String>> {
    state.controls.budget(true)?;
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    let Some(bindings) = &state.controls.bindings else {
        return Ok(Err("input bindings unavailable".into()));
    };
    let start = offset as usize;
    if start > bindings.entries.len()
        || limit == 0
        || limit as usize > mod_api::MAX_INPUT_BINDINGS_PER_READ
    {
        return Ok(Err("invalid input bindings page".into()));
    }
    if after == Some(bindings.revision) {
        return Ok(Ok(None));
    }
    let end = (start + limit as usize).min(bindings.entries.len());
    Ok(Ok(Some(InputBindings {
        revision: bindings.revision,
        total: bindings.total,
        entries: bindings.entries[start..end].to_vec(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{Declared, cinnabar::extension::input::Host as _};
    use crate::{BindingContext, BindingModifiers, InputActionBinding, ModGrants};

    /// A bounded physical binding snapshot that deliberately uses a rebound attack key.
    fn bindings() -> InputBindings {
        InputBindings {
            revision: 2,
            total: 1,
            entries: vec![InputActionBinding {
                action: "attack".into(),
                key: "KeyK".into(),
                context: BindingContext::Gameplay,
                modifiers: BindingModifiers {
                    shift: false,
                    control: false,
                    alt: false,
                    super_key: false,
                },
            }],
        }
    }

    /// Oversized or malformed host responses are refused before any guest can read them.
    #[test]
    fn binding_response_bounds_are_enforced() {
        let mut input = bindings();
        input.entries = vec![input.entries[0].clone(); mod_api::MAX_INPUT_BINDINGS];
        input.total = input.entries.len() as u32;
        assert!(validate(&input).is_ok());
        input.entries.push(input.entries[0].clone());
        assert!(validate(&input).is_err());
        input = bindings();
        input.entries[0].key = "x".repeat(mod_api::MAX_CONTROL_KEY_BYTES + 1);
        assert!(validate(&input).is_err());
        input = bindings();
        input.entries[0].action = "x".repeat(mod_api::MAX_INPUT_ACTION_BYTES + 1);
        assert!(validate(&input).is_err());
    }

    /// Controls consent gates full and unchanged reads; both consume the shared allowance.
    #[test]
    fn read_is_granted_cached_and_shares_controls_budget() {
        let mut state = State::new(ModGrants::default(), String::new(), Declared::default());
        state.controls.bindings = Some(Arc::new(bindings()));
        assert!(
            state
                .read_bindings(None, 0, mod_api::MAX_INPUT_BINDINGS_PER_READ as u32)
                .unwrap()
                .is_err()
        );
        state.grants.controls = true;
        state.controls.reset_budget();
        assert_eq!(
            state
                .read_bindings(None, 0, mod_api::MAX_INPUT_BINDINGS_PER_READ as u32)
                .unwrap()
                .unwrap(),
            Some(bindings())
        );
        assert_eq!(state.read_bindings(Some(2), 0, 1).unwrap().unwrap(), None);
        assert_eq!(
            state.read_bindings(Some(1), 0, 1).unwrap().unwrap(),
            Some(bindings())
        );
        for _ in 3..super::super::MAX_IMPORT_WRITES {
            state.read_controls().unwrap().unwrap();
        }
        assert!(state.read_bindings(Some(2), 0, 1).is_err());
    }
    /// Pages are bounded and stable, and invalid offsets never allocate an oversized reply.
    #[test]
    fn binding_pages_share_one_revision_and_validate_their_bounds() {
        let mut state = State::new(
            ModGrants {
                controls: true,
                ..Default::default()
            },
            String::new(),
            Declared::default(),
        );
        let mut full = bindings();
        full.entries = vec![full.entries[0].clone(); mod_api::MAX_INPUT_BINDINGS];
        full.total = full.entries.len() as u32;
        state.controls.bindings = Some(Arc::new(full));
        for offset in (0..mod_api::MAX_INPUT_BINDINGS).step_by(mod_api::MAX_INPUT_BINDINGS_PER_READ)
        {
            let page = state
                .read_bindings(
                    None,
                    offset as u32,
                    mod_api::MAX_INPUT_BINDINGS_PER_READ as u32,
                )
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(page.revision, 2);
            assert_eq!(page.total as usize, mod_api::MAX_INPUT_BINDINGS);
            assert_eq!(page.entries.len(), mod_api::MAX_INPUT_BINDINGS_PER_READ);
        }
        assert!(state.read_bindings(None, 0, 1).is_err());
        for (offset, limit) in [
            (129, 1),
            (0, 0),
            (0, mod_api::MAX_INPUT_BINDINGS_PER_READ as u32 + 1),
            (u32::MAX, 1),
        ] {
            state.controls.reset_budget();
            assert!(state.read_bindings(None, offset, limit).unwrap().is_err());
        }
        state.controls.reset_budget();
        let empty = state
            .read_bindings(None, mod_api::MAX_INPUT_BINDINGS as u32, 1)
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(empty.entries.is_empty());
    }

    /// The parsed current contract exposes physics and pose without importing action holds.
    #[test]
    fn current_records_contain_no_resolved_action_holds() {
        let mut resolve = wit_parser::Resolve::default();
        for path in [
            "../experience-sdk/wit/client/deps/server-experience",
            "../experience-sdk/wit/session",
            "../mod-api/wit",
        ] {
            resolve
                .push_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
                .unwrap();
        }
        let gameplay = resolve
            .interfaces
            .iter()
            .find(|(_, interface)| interface.name.as_deref() == Some("gameplay"))
            .unwrap()
            .1;
        for name in ["snapshot", "movement-snapshot"] {
            let wit_parser::TypeDefKind::Record(record) = &resolve.types[gameplay.types[name]].kind
            else {
                panic!("record")
            };
            assert!(record.fields.iter().all(|field| {
                !["attack-held", "jump-held", "sprint-held", "sneak-held"]
                    .contains(&field.name.as_str())
            }));
        }
    }
}
