use super::*;
use cinnabar::extension::{hud::Host as _, input::Host as _};

/// Initializes a looping guest so recovery tests exercise real component replacement.
fn fuel_guest(engine: &wasmtime::Engine, grants: ModGrants) -> Instance {
    Instance::new(
        engine,
        br#"(component
            (core module $m
                (func (export "init"))
                (func (export "frame") (loop $forever br $forever)))
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "frame") (canon lift (core func $i "frame"))))"#,
        grants,
        String::new(),
        Declared::default(),
        None,
    )
    .unwrap()
}

#[test]
fn fuel_recovery_preserves_queued_native_edits_and_unread_editor_saves() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut instance = fuel_guest(
        &engine,
        ModGrants {
            controls: true,
            hud: true,
            ..Default::default()
        },
    );
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

#[test]
fn fuel_recovery_keeps_screen_revision_epochs_and_discards_failed_screen_writes() {
    use cinnabar::extension::screen::Host as _;
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut instance = fuel_guest(
        &engine,
        ModGrants {
            screen: true,
            ..Default::default()
        },
    );
    instance
        .store
        .data_mut()
        .declared
        .actions
        .insert("edit".into());
    instance
        .store
        .data_mut()
        .set_text("edit".into(), "old".into())
        .unwrap()
        .unwrap();
    instance
        .store
        .data_mut()
        .focus_text("edit".into())
        .unwrap()
        .unwrap();
    commit(&mut instance.store);
    let applied = instance.screens().data.revision;
    instance
        .run(FRAME_FUEL, commit, |_, store| {
            store
                .data_mut()
                .set_text("edit".into(), "failed".into())?
                .unwrap();
            Err(wasmtime::Trap::OutOfFuel.into())
        })
        .unwrap_err();
    instance.recover_frame_fuel().unwrap();
    assert_eq!(instance.screens().data.texts["edit"].1, "old");
    instance
        .store
        .data_mut()
        .set_text("edit".into(), "new".into())
        .unwrap()
        .unwrap();
    instance
        .store
        .data_mut()
        .focus_text("edit".into())
        .unwrap()
        .unwrap();
    commit(&mut instance.store);
    assert!(instance.screens().data.texts["edit"].0 > applied);
    assert!(instance.screens().focus.as_ref().unwrap().0 > applied);
}

#[test]
fn fuel_recovery_keeps_touch_taps_and_trusted_drawing_observations() {
    use cinnabar::extension::touch::Host as _;
    use mod_api::drawing::{DrawingPermissions, DrawingSession, MaskDepth, MaskEntity};
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut instance = fuel_guest(
        &engine,
        ModGrants {
            controls: true,
            ..Default::default()
        },
    );
    instance
        .store
        .data_mut()
        .set_buttons(vec![cinnabar::extension::touch::Button {
            id: "menu".into(),
            label: "Menu".into(),
            anchor: cinnabar::extension::hud::Anchor::TopRight,
            offset: cinnabar::extension::hud::Point { x: 4., y: 24. },
            size: cinnabar::extension::hud::Point { x: 36., y: 18. },
        }])
        .unwrap()
        .unwrap();
    let actor = mod_render::retained::EntityKey {
        session: 7,
        dimension: 0,
        runtime_id: 9,
        spawn_revision: 1,
    };
    let target = MaskEntity {
        connection: actor.session,
        dimension: actor.dimension,
        runtime_id: actor.runtime_id,
        spawn_revision: actor.spawn_revision,
    };
    let render = &mut instance.store.data_mut().render;
    render.retained.bind_session(actor.session, actor.dimension);
    render
        .retained
        .reconcile_entities(vec![actor], &[])
        .unwrap();
    let old_entity = render.retained.observe_entities().unwrap()[0].0;
    render.masks.bind(DrawingSession {
        connection: actor.session,
        dimension: actor.dimension,
        local: true,
        permissions: DrawingPermissions::ALL,
    });
    render.masks.publish_targets(&[(target, 36)]);
    let old_target = render.masks.entity(target.runtime_id).unwrap();
    let old_mask = render.masks.create(MaskDepth::Test).unwrap();
    commit(&mut instance.store);
    let owner = instance.touch_owner();
    instance.frame(CallbackOutcome::Executed).unwrap_err();
    assert!(instance.queue_touch_press("menu"));
    instance.recover_frame_fuel().unwrap();
    assert_eq!(instance.touch_owner(), owner);
    let state = instance.store.data_mut();
    state.touch.begin_frame();
    assert_eq!(state.read_presses().unwrap().unwrap(), ["menu"]);
    let observed = state.render.retained.observe_entities().unwrap();
    assert_eq!(observed[0].1, actor);
    assert_ne!(observed[0].0, old_entity);
    let new_target = state.render.masks.entity(target.runtime_id).unwrap();
    assert_ne!(new_target, old_target);
    assert_eq!(
        state.render.masks.select(old_mask, &[new_target]),
        Err(mod_render::masks::MaskError::StaleHandle)
    );
}
