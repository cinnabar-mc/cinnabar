use super::*;
use ButtonState::{Pressed, Released};
use KeyCode::{KeyV, SuperLeft};

/// A modifier report attesting exactly `held` down.
fn report(held: &[KeyCode]) -> NativeStep {
    NativeStep::Modifiers(MODIFIERS.map(|key| Some(held.contains(&key))))
}

// macOS reports a Command held since before focus returned with the next key down, and Bevy
// appends the Command press it synthesizes after that key.
#[test]
fn synthesized_press_moves_before_the_key_it_was_reported_with() {
    let steps = [report(&[SuperLeft]), NativeStep::Key(KeyV, Pressed)];
    let batch = [(1, KeyV, Pressed), (2, SuperLeft, Pressed)];
    assert_eq!(place_edges(&steps, &batch), [(2, Some(1))]);
}

#[test]
fn real_modifier_press_after_a_key_keeps_its_place() {
    let steps = [
        NativeStep::Key(KeyV, Pressed),
        NativeStep::Key(SuperLeft, Pressed),
        report(&[SuperLeft]),
    ];
    let batch = [(1, KeyV, Pressed), (2, SuperLeft, Pressed)];
    assert!(place_edges(&steps, &batch).is_empty());
}

// A modifier key-up the OS consumed is reported after the key it was still held for.
#[test]
fn synthesized_release_reported_after_a_key_stays_after_it() {
    let steps = [
        report(&[SuperLeft]),
        NativeStep::Key(KeyV, Pressed),
        report(&[]),
    ];
    let batch = [(1, KeyV, Pressed), (2, SuperLeft, Released)];
    assert_eq!(place_edges(&steps, &batch), [(2, None)]);
    let steps = [report(&[]), NativeStep::Key(KeyV, Pressed)];
    assert_eq!(place_edges(&steps, &batch), [(2, Some(1))]);
}

#[test]
fn batches_without_native_reports_keep_their_order() {
    let batch = [(1, KeyV, Pressed), (2, SuperLeft, Pressed)];
    assert!(place_edges(&[], &batch).is_empty());
    let steps = [
        report(&[SuperLeft]),
        NativeStep::Key(KeyCode::KeyX, Pressed),
    ];
    assert!(place_edges(&steps, &batch).is_empty());
}

#[test]
fn native_order_puts_each_edge_before_its_native_key() {
    let window = Entity::PLACEHOLDER;
    let event = |key_code, state| KeyboardInput {
        key_code,
        logical_key: bevy::input::keyboard::Key::Unidentified(
            bevy::input::keyboard::NativeKey::Unidentified,
        ),
        state,
        text: None,
        repeat: false,
        window,
    };
    let mut messages = Messages::<KeyboardInput>::default();
    messages.write(event(KeyV, Pressed));
    messages.write(event(SuperLeft, Pressed));
    let mut cursor = MessageCursor::default();
    let batch: Vec<_> = cursor.read_with_id(&messages).collect();
    let mut order = ModifierEdgeOrder::default();
    order.record(
        &[report(&[SuperLeft]), NativeStep::Key(KeyV, Pressed)],
        batch.iter().copied(),
    );
    let keys: Vec<_> = order
        .native_order(batch.iter().copied())
        .into_iter()
        .map(|event| event.key_code)
        .collect();
    assert_eq!(keys, [SuperLeft, KeyV]);
}

#[test]
fn every_tracked_modifier_has_a_reported_side() {
    let modifiers = Modifiers::default();
    for key in MODIFIERS {
        assert_eq!(reported(&modifiers, key), Some(false), "{key:?}");
    }
}
