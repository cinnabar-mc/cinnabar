//! Bounded physical binding observations, shared across guest frame callbacks.

use super::State;
use crate::{InputBindingChange, InputBindings, ModHost};
use anyhow::{Result, ensure};
use std::sync::Arc;

/// One requested change to the player's own binding for a named action, committed by a
/// successful callback. The embedder validates it against its settings before applying it.
#[derive(Clone, Debug, PartialEq)]
pub struct InputBindingWrite {
    pub action: String,
    pub change: InputBindingChange,
}

/// Transactional player binding changes: staged by a callback, published on its success and
/// consumed once by the embedder. At most MAX_INPUT_BINDING_WRITES actions await application
/// at once, so every accepted write is kept until the embedder consumes it.
#[derive(Default)]
pub(crate) struct BindingWrites {
    pending: Vec<InputBindingWrite>,
    committed: Vec<InputBindingWrite>,
}

impl BindingWrites {
    /// Starts a callback with no staged writes. It sees the committed writes only to count
    /// their actions against the cap; the instance keeps ownership of them.
    pub(crate) fn transaction(&self) -> Self {
        Self {
            pending: Vec::new(),
            committed: self.committed.clone(),
        }
    }

    /// Takes a finished callback's staged writes.
    pub(crate) fn accept(&mut self, transaction: Self) {
        self.pending = transaction.pending;
    }

    /// Validates the request shape and keeps one change per action, the latest.
    fn stage(&mut self, action: String, change: InputBindingChange) -> Result<(), String> {
        if !action_name_valid(&action) {
            return Err("invalid input action name".into());
        }
        if let InputBindingChange::Assign(key) = &change
            && !mod_api::control_key_valid(key)
        {
            return Err("invalid physical key name".into());
        }
        let waiting = |name: &str| {
            self.pending
                .iter()
                .chain(&self.committed)
                .any(|write| write.action == name)
        };
        if !waiting(&action) {
            let mut actions: Vec<&str> = self
                .pending
                .iter()
                .chain(&self.committed)
                .map(|write| write.action.as_str())
                .collect();
            actions.sort_unstable();
            actions.dedup();
            if actions.len() >= mod_api::MAX_INPUT_BINDING_WRITES {
                return Err("too many input binding writes awaiting application".into());
            }
        }
        merge(&mut self.pending, InputBindingWrite { action, change });
        Ok(())
    }

    /// Publishes a successful callback's writes, replacing unconsumed ones for the same action.
    /// Staging already bounded the actions, so nothing accepted is dropped.
    pub(crate) fn commit(&mut self) {
        for write in std::mem::take(&mut self.pending) {
            merge(&mut self.committed, write);
        }
    }

    /// Drops a failed callback's staged writes.
    pub(crate) fn rollback(&mut self) {
        self.pending.clear();
    }

    /// Consumes the committed writes in staging order.
    pub(crate) fn take(&mut self) -> Vec<InputBindingWrite> {
        std::mem::take(&mut self.committed)
    }
}

/// Replaces the write for the same action, or appends it.
fn merge(writes: &mut Vec<InputBindingWrite>, write: InputBindingWrite) {
    if let Some(existing) = writes.iter_mut().find(|old| old.action == write.action) {
        *existing = write;
    } else {
        writes.push(write);
    }
}

/// Whether an action name fits the stable kebab-case binding name contract.
fn action_name_valid(action: &str) -> bool {
    !action.is_empty()
        && action.len() <= mod_api::MAX_INPUT_ACTION_BYTES
        && action
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

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
            action_name_valid(&binding.action) && mod_api::control_key_valid(&binding.key)
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

/// Stages a player binding change under controls authority and the controls write budget.
pub(super) fn write(
    state: &mut State,
    action: String,
    change: InputBindingChange,
) -> Result<Result<(), String>> {
    state.controls.budget(false)?;
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    Ok(state.controls.binding_writes.stage(action, change))
}

impl ModHost {
    /// Consumes player binding changes committed by successful callbacks of an active mod
    /// that still holds controls authority.
    pub fn take_input_binding_writes(&mut self) -> Vec<InputBindingWrite> {
        let allowed = self.is_active() && self.grants().controls;
        let writes = self
            .instance
            .store
            .data_mut()
            .controls
            .binding_writes
            .take();
        if allowed { writes } else { Vec::new() }
    }
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

    /// Binding writes need controls, validate their shape, keep the latest change per action
    /// and publish only after a successful callback.
    #[test]
    fn binding_writes_are_granted_validated_and_transactional() {
        let mut state = State::new(ModGrants::default(), String::new(), Declared::default());
        let assign = |key: &str| InputBindingChange::Assign(key.into());
        assert!(
            state
                .write_binding("freelook".into(), assign("KeyG"))
                .unwrap()
                .is_err()
        );
        state.grants.controls = true;
        state.controls.reset_budget();
        for (action, change) in [
            ("", InputBindingChange::Reset),
            ("Freelook", InputBindingChange::Reset),
            ("freelook", assign("")),
            ("freelook", assign("Key G")),
        ] {
            assert!(state.write_binding(action.into(), change).unwrap().is_err());
        }
        state.controls.reset_budget();
        state
            .write_binding("freelook".into(), assign("KeyG"))
            .unwrap()
            .unwrap();
        state.controls.rollback();
        state.controls.commit();
        assert!(state.controls.binding_writes.take().is_empty());
        state
            .write_binding("freelook".into(), assign("KeyG"))
            .unwrap()
            .unwrap();
        state
            .write_binding("jump".into(), InputBindingChange::Unbind)
            .unwrap()
            .unwrap();
        state
            .write_binding("freelook".into(), InputBindingChange::Reset)
            .unwrap()
            .unwrap();
        state.controls.commit();
        assert_eq!(
            state.controls.binding_writes.take(),
            [
                InputBindingWrite {
                    action: "freelook".into(),
                    change: InputBindingChange::Reset,
                },
                InputBindingWrite {
                    action: "jump".into(),
                    change: InputBindingChange::Unbind,
                },
            ]
        );
        assert!(state.controls.binding_writes.take().is_empty());
    }

    /// At most MAX_INPUT_BINDING_WRITES actions await application, across callbacks that
    /// commit before the embedder consumes them, and revocation drops unconsumed writes.
    #[test]
    fn binding_writes_are_bounded_and_revoked() {
        let mut writes = BindingWrites::default();
        for index in 0..mod_api::MAX_INPUT_BINDING_WRITES {
            writes
                .stage(format!("hotbar-{index}"), InputBindingChange::Unbind)
                .unwrap();
        }
        assert!(
            writes
                .stage("freelook".into(), InputBindingChange::Unbind)
                .is_err()
        );
        writes
            .stage("hotbar-0".into(), InputBindingChange::Reset)
            .unwrap();
        writes.commit();
        assert_eq!(writes.committed.len(), mod_api::MAX_INPUT_BINDING_WRITES);
        // A later callback sees the unconsumed writes: it may replace one, not add another.
        let mut later = writes.transaction();
        assert!(
            later
                .stage("freelook".into(), InputBindingChange::Unbind)
                .is_err()
        );
        later
            .stage("hotbar-1".into(), InputBindingChange::Reset)
            .unwrap();
        writes.accept(later);
        writes.commit();
        assert_eq!(writes.committed.len(), mod_api::MAX_INPUT_BINDING_WRITES);
        assert_eq!(writes.take().len(), mod_api::MAX_INPUT_BINDING_WRITES);
        writes
            .stage("freelook".into(), InputBindingChange::Unbind)
            .unwrap();
        let mut state = State::new(
            ModGrants {
                controls: true,
                ..Default::default()
            },
            String::new(),
            Declared::default(),
        );
        state
            .write_binding("freelook".into(), InputBindingChange::Unbind)
            .unwrap()
            .unwrap();
        state.controls.commit();
        state.controls.revoke();
        assert!(state.controls.binding_writes.take().is_empty());
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
