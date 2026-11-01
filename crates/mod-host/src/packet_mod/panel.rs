//! Generic grant-gated panel state for core packet reactors.
use super::core::State;
use anyhow::{Context, Result};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use ui::mod_panel::Panel;
use wasmtime::{Caller, Linker, Memory};

/// A validated surface update, retained until replaced by its owner.
#[derive(Clone, Default)]
pub struct Output {
    pub panel: Option<Arc<Panel>>,
    pub open: bool,
    pub revision: u64,
    pub open_order: u64,
}
#[derive(Default)]
struct Mailbox {
    output: Output,
    input: mod_api::panel_input::Input,
}
/// A session-local mailbox; all guest execution stays on the packet worker.
#[derive(Clone)]
pub struct PanelHandle {
    identity: u64,
    enabled: bool,
    inner: Arc<Mutex<Mailbox>>,
    active: Arc<AtomicBool>,
}
impl PanelHandle {
    /// Creates a mailbox with immutable controls authority.
    pub(super) fn new(enabled: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            identity: NEXT.fetch_add(1, Ordering::Relaxed),
            enabled,
            inner: Default::default(),
            active: Arc::new(AtomicBool::new(true)),
        }
    }
    /// Stable presentation owner for this instance, independent of world publication timing.
    pub fn identity(&self) -> u64 {
        self.identity
    }
    /// Whether controls were granted to this instance.
    pub fn enabled(&self) -> bool {
        self.enabled && self.active.load(Ordering::Acquire)
    }
    /// Identifies the session without exposing its source or relying on UI session timing.
    pub fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
    /// Revokes surface and input access when the instance stops or is quarantined.
    pub fn deactivate(&self) {
        self.active.store(false, Ordering::Release);
    }
    /// Reads the retained surface without changing pending physical input.
    pub fn output(&self) -> Output {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .output
            .clone()
    }
    /// Publishes current physical state and retains bounded edges until the guest reads them.
    pub fn exchange(&self, mut input: mod_api::panel_input::Input) -> Output {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        input.pointer_cancelled |= state.input.pointer_cancelled;
        input.fields.splice(0..0, state.input.fields.drain(..));
        input.fields.truncate(ui::mod_panel::MAX_PANEL_EVENTS);
        input.keys.splice(0..0, state.input.keys.drain(..));
        input.events.splice(0..0, state.input.events.drain(..));
        input.keys.truncate(mod_api::MAX_CONTROL_KEYS);
        input.events.truncate(ui::mod_panel::MAX_PANEL_EVENTS);
        input.held.truncate(mod_api::MAX_CONTROL_KEYS);
        state.input = input;
        state.output.clone()
    }
    /// Applies standard UI navigation without asking the guest to close itself.
    pub fn close(&self) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        state.output.open = false;
    }
}
/// Links narrow calls without exposing files, resolved game actions or menu policy.
pub(super) fn link(linker: &mut Linker<State>) -> Result<()> {
    use mod_api::panel_input::{DENIED, INVALID, NO_CONTENT};
    linker.func_wrap(
        "cinnabar:extension/panel",
        "set-content",
        |mut caller: Caller<'_, State>, pointer: i32, length: i32| -> Result<i32> {
            if !caller.data().panel.enabled() {
                return Ok(DENIED);
            }
            let Some(bytes) =
                payload(&mut caller, pointer, length, ui::mod_panel::MAX_PANEL_BYTES)?
            else {
                return Ok(INVALID);
            };
            let panel = if bytes.is_empty() {
                None
            } else {
                let Ok(panel) = serde_json::from_slice::<Panel>(&bytes) else {
                    return Ok(INVALID);
                };
                if panel.validate().is_err() || !surface_only(&panel) {
                    return Ok(INVALID);
                }
                Some(Arc::new(panel))
            };
            let mut state = caller
                .data()
                .panel
                .inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.output.panel = panel;
            if state.output.panel.is_none() {
                state.output.open = false;
            }
            state.output.revision = state.output.revision.wrapping_add(1);
            Ok(0)
        },
    )?;
    linker.func_wrap(
        "cinnabar:extension/panel",
        "set-open",
        |caller: Caller<'_, State>, open: i32| -> Result<i32> {
            if !caller.data().panel.enabled() {
                return Ok(DENIED);
            }
            if !matches!(open, 0 | 1) {
                return Ok(INVALID);
            }
            let mut state = caller
                .data()
                .panel
                .inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if open != 0 && state.output.panel.is_none() {
                return Ok(NO_CONTENT);
            }
            state.output.open = open != 0;
            if open != 0 {
                state.output.open_order = crate::panel_order::next_panel_open_order();
            }
            state.output.revision = state.output.revision.wrapping_add(1);
            Ok(0)
        },
    )?;
    linker.func_wrap(
        "cinnabar:extension/panel",
        "update-content",
        |mut caller: Caller<'_, State>, pointer: i32, length: i32| -> Result<i32> {
            if !caller.data().panel.enabled() {
                return Ok(DENIED);
            }
            let Some(bytes) = payload(
                &mut caller,
                pointer,
                length,
                ui::mod_panel::MAX_PANEL_PATCH_BYTES,
            )?
            else {
                return Ok(INVALID);
            };
            let Ok(json) = std::str::from_utf8(&bytes) else {
                return Ok(INVALID);
            };
            let Ok(patch) = ui::mod_panel::PanelPatch::parse(json) else {
                return Ok(INVALID);
            };
            let current = caller
                .data()
                .panel
                .inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .output
                .panel
                .clone();
            let Some(current) = current else {
                return Ok(NO_CONTENT);
            };
            let Ok(panel) = patch.apply(&current, &Default::default()) else {
                return Ok(INVALID);
            };
            if !surface_only(&panel) {
                return Ok(INVALID);
            }
            let mut state = caller
                .data()
                .panel
                .inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.output.panel = Some(Arc::new(panel));
            state.output.revision = state.output.revision.wrapping_add(1);
            Ok(0)
        },
    )?;
    linker.func_wrap(
        "cinnabar:extension/input",
        "read",
        |mut caller: Caller<'_, State>, pointer: i32, capacity: i32| -> Result<i32> {
            if !caller.data().panel.enabled() {
                return Ok(DENIED);
            }
            if capacity < 0 {
                return Ok(INVALID);
            }
            let handle = caller.data().panel.clone();
            let mut state = handle.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let bytes = serde_json::to_vec(&state.input)?;
            let length = i32::try_from(bytes.len())?;
            if capacity < length {
                return Ok(length);
            }
            memory(&mut caller)?.write(&mut caller, usize::try_from(pointer)?, &bytes)?;
            state.input.keys.clear();
            state.input.events.clear();
            state.input.fields.clear();
            state.input.pointer_cancelled = false;
            Ok(length)
        },
    )?;
    Ok(())
}

/// Reads a bounded request; malformed lengths reject the request and invalid pointers trap.
fn payload(
    caller: &mut Caller<'_, State>,
    pointer: i32,
    length: i32,
    maximum: usize,
) -> Result<Option<Vec<u8>>> {
    let Ok(length) = usize::try_from(length) else {
        return Ok(None);
    };
    if length > maximum {
        return Ok(None);
    }
    let mut bytes = vec![0; length];
    memory(caller)?.read(&*caller, usize::try_from(pointer)?, &mut bytes)?;
    Ok(Some(bytes))
}

/// Surface-only authority does not expose actor observations to the retained panel.
fn surface_only(panel: &Panel) -> bool {
    panel
        .surface
        .bindings
        .values()
        .all(|value| value.projection().and_then(|p| p.actor()).is_none())
}

/// Requires the reactor's exported linear memory for every bounded transfer.
fn memory(caller: &mut Caller<'_, State>) -> Result<Memory> {
    caller
        .get_export("memory")
        .and_then(wasmtime::Extern::into_memory)
        .context("packet memory missing")
}

#[cfg(test)]
mod tests {
    use crate::{
        ModGrants,
        packet_mod::{CompiledPacketMod, SessionIdentity},
    };

    /// Starts a reactor which publishes and opens an ordinary JSON-UI surface through imports.
    fn reactor(controls: bool, panel: &str, expected: (i32, i32)) -> crate::packet_mod::PacketMod {
        let encoded: String = panel.bytes().map(|byte| format!("\\{byte:02x}")).collect();
        let imports = format!(
            r#"
          (import "cinnabar:extension/panel" "set-content" (func $content (param i32 i32) (result i32)))
          (import "cinnabar:extension/panel" "set-open" (func $open (param i32) (result i32)))
          (data (i32.const 4096) "{encoded}")"#
        );
        let start = format!(
            "(if (i32.ne (call $content (i32.const 4096) (i32.const {})) (i32.const {})) (then unreachable)) (if (i32.ne (call $open (i32.const 1)) (i32.const {})) (then unreachable))",
            panel.len(),
            expected.0,
            expected.1
        );
        let bytes = wat::parse_str(
            include_str!("tests/guest.wat")
                .replace("$IMPORTS", &imports)
                .replace("$START", &start),
        )
        .unwrap();
        CompiledPacketMod::compile(
            &bytes,
            &ModGrants {
                packets: true,
                controls,
                ..Default::default()
            },
        )
        .unwrap()
        .instantiate()
        .unwrap()
    }
    /// Supplies a minimal original surface shared by the panel validation path.
    fn surface() -> String {
        serde_json::json!({"controls":[],"surface":{"screen":"test.root","document":"{\"namespace\":\"test\",\"root\":{\"type\":\"panel\"}}"}}).to_string()
    }
    #[test]
    fn reactor_publishes_a_generic_panel_without_a_menu_export() {
        let mut guest = reactor(true, &surface(), (0, 0));
        guest
            .start(
                &SessionIdentity {
                    display_name: "Fixture".into(),
                    xuid: "1".into(),
                    uuid: "fixture".into(),
                },
                "{}",
                &[],
            )
            .unwrap();
        let handle = guest.panel().unwrap();
        let output = handle.exchange(Default::default());
        assert!(output.open && output.panel.is_some());
        handle.close();
        assert!(!handle.exchange(Default::default()).open);
        guest.stop();
        assert!(!handle.enabled());
    }
    #[test]
    fn denied_and_invalid_surface_calls_preserve_packet_processing() {
        for (granted, panel, expected) in
            [(false, surface(), (-1, -1)), (true, "{}".into(), (-2, -3))]
        {
            let mut guest = reactor(granted, &panel, expected);
            guest
                .start(
                    &SessionIdentity {
                        display_name: "Fixture".into(),
                        xuid: "1".into(),
                        uuid: "fixture".into(),
                    },
                    "{}",
                    &[],
                )
                .unwrap();
            assert!(guest.is_active());
            if let Some(handle) = guest.panel() {
                assert!(handle.output().panel.is_none());
            }
            assert_eq!(
                guest
                    .on_batch(crate::packet_mod::Direction::FromClient, &[1])
                    .unwrap(),
                Some(vec![0])
            );
        }
    }

    #[test]
    fn raw_input_size_queries_preserve_edges_and_successful_reads_consume_them() {
        let mut source = include_str!("tests/guest.wat")
            .replace("$START", "")
            .replace(
                "$IMPORTS",
                r#"
            (import "cinnabar:extension/input" "read" (func $read (param i32 i32) (result i32)))
            (global $required (mut i32) (i32.const 0))"#,
            );
        let start = source.find("  (func (export \"tick\")").unwrap();
        let end = source.find("  (func (export \"stop\")").unwrap();
        source.replace_range(
            start..end,
            r#"  (func (export "tick") (result i32)
            (global.set $required (call $read (i32.const -1) (i32.const 0)))
            (if (i32.lt_s (global.get $required) (i32.const 0)) (then
                (if (i32.ne (global.get $required) (i32.const -1)) (then unreachable))
                (i32.store (i32.const 128) (i32.const 0))
                (i32.store (i32.const 132) (i32.const 0))
                (i32.store (i32.const 136) (i32.const 0))
                (i32.store (i32.const 140) (i32.const 0))
                (i32.store (i32.const 144) (i32.const 0))
                (return (i32.const 128))))
            (if (i32.ne (call $read (i32.const -1) (i32.const -1)) (i32.const -2)) (then unreachable))
            (i32.store (i32.const 1020) (call $read (i32.const 8192) (global.get $required)))
            (if (i32.ne (i32.load (i32.const 1020)) (global.get $required)) (then unreachable))
            (i32.store (i32.const 128) (i32.const 0))
            (i32.store (i32.const 132) (i32.const 8192))
            (i32.store (i32.const 136) (global.get $required))
            (i32.store (i32.const 140) (i32.const 0))
            (i32.store (i32.const 144) (i32.const 0))
            (i32.const 128))
        "#,
        );
        let bytes = wat::parse_str(source).unwrap();
        let compiled = CompiledPacketMod::compile(
            &bytes,
            &ModGrants {
                packets: true,
                controls: true,
                ..Default::default()
            },
        )
        .unwrap();
        let mut guest = compiled.instantiate().unwrap();
        guest
            .panel()
            .unwrap()
            .exchange(mod_api::panel_input::Input {
                focused: true,
                held: vec!["KeyR".into()],
                keys: vec![mod_api::panel_input::Key {
                    key: "KeyR".into(),
                    text: Some("r".into()),
                }],
                ..Default::default()
            });
        let first: mod_api::panel_input::Input =
            serde_json::from_slice(&guest.tick().unwrap().to_client).unwrap();
        assert_eq!(first.keys[0].key, "KeyR");
        let next: mod_api::panel_input::Input =
            serde_json::from_slice(&guest.tick().unwrap().to_client).unwrap();
        assert!(next.keys.is_empty());
        assert_eq!(next.held, ["KeyR"]);
        let mut denied = CompiledPacketMod::compile(
            &bytes,
            &ModGrants {
                packets: true,
                ..Default::default()
            },
        )
        .unwrap()
        .instantiate()
        .unwrap();
        assert!(denied.tick().unwrap().to_client.is_empty());
        assert!(denied.is_active());
    }
}
