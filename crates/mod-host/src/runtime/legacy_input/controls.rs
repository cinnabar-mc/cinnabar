//! Frozen controls records for components built before physical admission was exposed.

use super::*;
use crate::runtime::cinnabar::extension::{input, panel};

#[derive(ComponentType, Lower, Lift)]
#[component(record)]
struct Controls {
    seconds: f32,
    focused: bool,
    gameplay: bool,
    #[component(name = "panel-open")]
    panel_open: bool,
    #[component(name = "keys-pressed")]
    keys_pressed: Vec<String>,
    #[component(name = "keys-held")]
    keys_held: Vec<String>,
    events: Vec<panel::Event>,
    pointer: Option<(f32, f32)>,
    wheel: f32,
}

#[derive(ComponentType, Lower, Lift)]
#[component(record)]
struct Selection {
    #[component(name = "keys-pressed")]
    keys_pressed: Option<Vec<String>>,
    #[component(name = "keys-held")]
    keys_held: Option<Vec<String>>,
    events: bool,
}

impl From<input::Controls> for Controls {
    /// Drops only the new observation while preserving the original canonical layout.
    fn from(value: input::Controls) -> Self {
        Self {
            seconds: value.seconds,
            focused: value.focused,
            gameplay: value.gameplay,
            panel_open: value.panel_open,
            keys_pressed: value.keys_pressed,
            keys_held: value.keys_held,
            events: value.events,
            pointer: value.pointer,
            wheel: value.wheel,
        }
    }
}

/// Installs old signatures only for imports whose returned record lacks keys-admitted.
pub(super) fn link(
    engine: &Engine,
    component: &Component,
    linker: &mut Linker<State>,
) -> Result<()> {
    for (name, item) in component.component_type().imports(engine) {
        if !name.starts_with("cinnabar:extension/input@") {
            continue;
        }
        let ComponentItem::ComponentInstance(instance) = item else {
            continue;
        };
        let old = |method| {
            let Some(ComponentItem::ComponentFunc(func)) = instance.get_export(engine, method)
            else {
                return false;
            };
            let Some(Type::Result(result)) = func.results().next() else {
                return false;
            };
            let Some(Type::Record(record)) = result.ok() else {
                return false;
            };
            !record.fields().any(|field| field.name == "keys-admitted")
        };
        let read = old("read-controls");
        let selected = old("read-selected-controls");
        if !(read || selected) {
            continue;
        }
        linker.allow_shadowing(true);
        let mut imports = linker.instance(name)?;
        macro_rules! forward {
            ($wire:literal, $method:ident $(, $arg:ident : $ty:ty)*) => {
                imports.func_wrap($wire, |mut store: StoreContextMut<'_, State>, ($($arg,)*): ($($ty,)*)| {
                    Ok((input::Host::$method(store.data_mut(), $($arg,)*)?,))
                })?;
            };
        }
        forward!("demo-pressed", demo_pressed);
        forward!("read-text-events", read_text_events);
        forward!("read-field-events", read_field_events);
        forward!("reserve-keys", reserve_keys, keys: Vec<String>);
        forward!("read-bindings", read_bindings, after: Option<u64>, offset: u32, limit: u32);
        if read {
            imports.func_wrap(
                "read-controls",
                |mut store: StoreContextMut<'_, State>, (): ()| {
                    Ok((input::Host::read_controls(store.data_mut())?.map(Controls::from),))
                },
            )?;
        } else {
            forward!("read-controls", read_controls);
        }
        if selected {
            imports.func_wrap(
                "read-selected-controls",
                |mut store: StoreContextMut<'_, State>, (selection,): (Selection,)| {
                    Ok((input::Host::read_selected_controls(
                        store.data_mut(),
                        input::Selection {
                            keys_pressed: selection.keys_pressed,
                            keys_held: selection.keys_held,
                            keys_admitted: Some(Vec::new()),
                            events: selection.events,
                        },
                    )?
                    .map(Controls::from),))
                },
            )?;
        } else {
            forward!("read-selected-controls", read_selected_controls, selection: input::Selection);
        }
        linker.allow_shadowing(false);
    }
    Ok(())
}

#[cfg(test)]
#[path = "controls_tests.rs"]
mod tests;
