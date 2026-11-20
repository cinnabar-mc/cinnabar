//! Puts modifier edges that Bevy synthesizes at the end of a key batch back where the platform
//! reported them.
//!
//! bevy_winit buffers winit's `ModifiersChanged` and, after the batch's native key events,
//! synthesizes `KeyboardInput` edges for modifiers whose own key event never arrived. On macOS a
//! Command held while the window regained focus is first reported with the next key press, so
//! the synthesized Command press trails the V it modified. The raw winit stream keeps the native
//! order, which tells that press apart from a real Command press after V.

use super::chat_modifiers::MODIFIERS;
use bevy::{
    ecs::message::{MessageCursor, MessageId, Messages},
    input::{ButtonState, keyboard::KeyboardInput},
    prelude::{Entity, KeyCode, Local, Res, ResMut, Resource},
    winit::{RawWinitWindowEvent, converters::convert_keyboard_input},
};
use winit::{
    event::{Modifiers, WindowEvent},
    keyboard::ModifiersKeyState,
};

/// One native event of a key batch, in the order the platform delivered it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum NativeStep {
    /// A native key event, as Bevy forwards it.
    Key(KeyCode, ButtonState),
    /// Reported modifier state per entry of [`MODIFIERS`]: attested held (`Some(true)`),
    /// attested up (`Some(false)`) or unknown.
    Modifiers([Option<bool>; MODIFIERS.len()]),
}

/// Where each synthesized modifier edge of the latest key batch belongs.
#[derive(Resource, Default)]
pub(crate) struct ModifierEdgeOrder {
    /// `(edge message id, id of the native key event it precedes)`; `None` keeps it last.
    edges: Vec<(usize, Option<usize>)>,
}

impl ModifierEdgeOrder {
    /// Records a key batch: `steps` from the platform and `batch` as Bevy wrote it.
    pub(crate) fn record<'a>(
        &mut self,
        steps: &[NativeStep],
        batch: impl IntoIterator<Item = (&'a KeyboardInput, MessageId<KeyboardInput>)>,
    ) {
        let batch: Vec<_> = batch
            .into_iter()
            .map(|(event, id)| (id.id, event.key_code, event.state))
            .collect();
        self.edges = place_edges(steps, &batch);
    }

    /// `events` in the order the platform delivered them: each synthesized modifier edge moves to
    /// just before the first native key event reported after it. Other events keep their order.
    pub(crate) fn native_order<'a>(
        &self,
        events: impl IntoIterator<Item = (&'a KeyboardInput, MessageId<KeyboardInput>)>,
    ) -> Vec<KeyboardInput> {
        let mut edges = Vec::new();
        let mut natives = Vec::new();
        for (event, id) in events {
            match self.edges.iter().find(|(edge, _)| *edge == id.id) {
                Some(&(_, before)) => edges.push((before, event)),
                None => natives.push((id.id, event)),
            }
        }
        let mut ordered = Vec::with_capacity(edges.len() + natives.len());
        for (id, event) in natives {
            ordered.extend(
                edges
                    .extract_if(.., |(before, _)| *before == Some(id))
                    .map(|(_, edge)| edge.clone()),
            );
            ordered.push(event.clone());
        }
        ordered.extend(edges.into_iter().map(|(_, edge)| edge.clone()));
        ordered
    }
}

/// Records where this frame's synthesized modifier edges belong among its native key events.
pub(crate) fn observe_key_batch(
    native: Option<Res<Messages<RawWinitWindowEvent>>>,
    keyboard: Option<Res<Messages<KeyboardInput>>>,
    mut cursors: Local<(
        MessageCursor<RawWinitWindowEvent>,
        MessageCursor<KeyboardInput>,
    )>,
    mut order: ResMut<ModifierEdgeOrder>,
) {
    let (native_cursor, key_cursor) = &mut *cursors;
    let steps: Vec<_> = native.map_or_else(Vec::new, |native| {
        native_cursor
            .read(&native)
            .filter_map(native_step)
            .collect()
    });
    match keyboard {
        Some(keyboard) => order.record(&steps, key_cursor.read_with_id(&keyboard)),
        None => order.record(&steps, []),
    }
}

/// The key and modifier events of a raw winit event; winit's own synthetic key events, which
/// Bevy drops, are skipped.
fn native_step(event: &RawWinitWindowEvent) -> Option<NativeStep> {
    match &event.event {
        WindowEvent::KeyboardInput {
            event,
            is_synthetic: false,
            ..
        } => {
            let input = convert_keyboard_input(event, Entity::PLACEHOLDER);
            Some(NativeStep::Key(input.key_code, input.state))
        }
        WindowEvent::ModifiersChanged(modifiers) => Some(NativeStep::Modifiers(
            MODIFIERS.map(|key| reported(modifiers, key)),
        )),
        _ => None,
    }
}

/// Whether `modifiers` attests `key` held or up. A side the platform cannot attest is up only
/// when no key of its group is held.
fn reported(modifiers: &Modifiers, key: KeyCode) -> Option<bool> {
    let (side, group) = side_state(modifiers, key)?;
    match side {
        ModifiersKeyState::Pressed => Some(true),
        ModifiersKeyState::Unknown if group => None,
        _ => Some(false),
    }
}

/// The reported state of `key`'s side and whether any key of its group is held, or `None` for a
/// key outside [`MODIFIERS`].
fn side_state(modifiers: &Modifiers, key: KeyCode) -> Option<(ModifiersKeyState, bool)> {
    let state = modifiers.state();
    Some(match key {
        KeyCode::ControlLeft => (modifiers.lcontrol_state(), state.control_key()),
        KeyCode::ControlRight => (modifiers.rcontrol_state(), state.control_key()),
        KeyCode::SuperLeft => (modifiers.lsuper_state(), state.super_key()),
        KeyCode::SuperRight => (modifiers.rsuper_state(), state.super_key()),
        KeyCode::AltLeft => (modifiers.lalt_state(), state.alt_key()),
        KeyCode::AltRight => (modifiers.ralt_state(), state.alt_key()),
        KeyCode::ShiftLeft => (modifiers.lshift_state(), state.shift_key()),
        KeyCode::ShiftRight => (modifiers.rshift_state(), state.shift_key()),
        _ => return None,
    })
}

/// For each synthesized edge of `batch` (`(message id, key, state)` in write order), the id of
/// the first native key event after the platform reported that edge's state, or `None` when it
/// stays last. Bevy writes a batch's native key events in `steps` order and then its synthesized
/// modifier edges; a batch that does not have that shape, such as driven input, is left alone.
fn place_edges(
    steps: &[NativeStep],
    batch: &[(usize, KeyCode, ButtonState)],
) -> Vec<(usize, Option<usize>)> {
    let natives: Vec<_> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| matches!(step, NativeStep::Key(..)).then_some(index))
        .collect();
    let aligned = batch.len() >= natives.len()
        && natives.iter().zip(batch).all(|(&index, &(_, key, state))| {
            matches!(steps[index], NativeStep::Key(native, native_state) if native == key && native_state == state)
        });
    let reports = steps
        .iter()
        .any(|step| matches!(step, NativeStep::Modifiers(_)));
    let tail = &batch[natives.len().min(batch.len())..];
    if !aligned || !reports || tail.iter().any(|(_, key, _)| !MODIFIERS.contains(key)) {
        return Vec::new();
    }
    tail.iter()
        .filter_map(|&(id, key, state)| {
            let slot = MODIFIERS.iter().position(|modifier| *modifier == key)?;
            let held = state == ButtonState::Pressed;
            let report = |step: &NativeStep| match step {
                NativeStep::Modifiers(report) => report[slot],
                NativeStep::Key(..) => None,
            };
            // The edge took effect at the first report of its final state after the last report
            // of the opposite state.
            let start = steps
                .iter()
                .rposition(|step| report(step) == Some(!held))
                .map_or(0, |index| index + 1);
            let Some(reported_at) = steps[start..]
                .iter()
                .position(|step| report(step) == Some(held))
            else {
                return Some((id, None));
            };
            let before = natives
                .iter()
                .position(|&index| index > start + reported_at)
                .map(|native| batch[native].0);
            Some((id, before))
        })
        .collect()
}

#[cfg(test)]
mod tests;
