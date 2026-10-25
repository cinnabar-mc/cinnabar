use super::*;
use cinnabar::extension::{hud::Host as _, input::Host as _};

#[test]
fn fuel_recovery_preserves_queued_native_edits_and_unread_editor_saves() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut instance = Instance::new(
        &engine,
        br#"(component
            (core module $m
                (func (export "init"))
                (func (export "frame") (loop $forever br $forever)))
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "frame") (canon lift (core func $i "frame"))))"#,
        ModGrants {
            controls: true,
            hud: true,
            ..Default::default()
        },
        String::new(),
        Declared::default(),
        None,
    )
    .unwrap();
    cinnabar::extension::panel::Host::set_content(instance.store.data_mut(), r#"{"title":"Text","toggle_key":"ShiftRight","dark":true,"controls":[{"kind":"text","id":"name","label":"Name","value":"Main","min_chars":1,"max_chars":8,"live":true}]}"#.into()).unwrap().unwrap();
    commit(&mut instance.store);
    instance.frame(CallbackOutcome::Executed).unwrap_err();
    instance
        .queue_text_events(vec![ui::mod_panel::TextEvent {
            id: "name".into(),
            value: "Edited".into(),
        }])
        .unwrap();
    instance
        .queue_field_events(vec![ui::mod_panel::FieldEvent::Change(
            ui::mod_panel::TextEvent {
                id: "name".into(),
                value: "Live".into(),
            },
        )])
        .unwrap();
    instance.deliver_hud_editor_result(ui::mod_hud::EditorResult {
        saved: true,
        reset: true,
        placements: Vec::new(),
        hidden_cards: Vec::new(),
        replace_pending_reset: false,
    });
    assert!(instance.recover_frame_fuel().unwrap());
    assert!(
        matches!(&instance.panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == "Live")
    );
    let state = instance.store.data_mut();
    state.controls.begin_text_frame();
    assert_eq!(
        state.read_text_events().unwrap().unwrap()[0].value,
        "Edited"
    );
    assert!(
        matches!(&state.read_field_events().unwrap().unwrap()[0], cinnabar::extension::input::FieldEvent::Change(event) if event.value == "Live")
    );
    let saved = state.read_editor_result().unwrap().unwrap().unwrap();
    assert!(saved.saved && saved.reset);
    commit(&mut instance.store);
    assert!(
        instance
            .store
            .data_mut()
            .read_editor_result()
            .unwrap()
            .unwrap()
            .is_none()
    );
}
