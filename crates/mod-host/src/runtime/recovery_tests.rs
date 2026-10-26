use super::*;
use cinnabar::extension::input::Host as _;

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
fn fuel_recovery_discards_package_text_queued_for_the_old_guest() {
    let source = br#"(component
        (core module $m
            (memory (export "memory") 1)
            (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 0)
            (func (export "init"))
            (func (export "frame") (loop $forever br $forever))
            (func (export "text-changed") (param i32 i32 i32 i32) unreachable))
        (core instance $i (instantiate $m))
        (alias core export $i "memory" (core memory $memory))
        (alias core export $i "realloc" (core func $realloc))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame")))
        (func (export "text-changed") (param "control" string) (param "text" string)
            (canon lift (core func $i "text-changed")
                (memory $memory) (realloc $realloc))))"#;
    let directory = tempfile::tempdir().unwrap();
    let mut host = crate::ModHost::load_snapshot_with_grants(
        &directory.path().join("package-edits.wat"),
        source,
        ModGrants::default(),
    )
    .unwrap();
    host.instance
        .store
        .data_mut()
        .declared
        .actions
        .insert("edit".into());
    host.frame(false).unwrap_err();
    let edit = crate::ModEvent::TextChanged {
        control: "edit".into(),
        text: "old".into(),
    };
    assert_eq!(
        host.dispatch(vec![edit.clone()]).unwrap(),
        CallbackOutcome::Deferred
    );
    host.frame(false).unwrap();
    assert!(host.is_active());
    host.dispatch(Vec::new()).unwrap();
    assert!(host.is_active());
    // A fresh edit still reaches the callback, whose trap proves it was not suppressed.
    host.dispatch(vec![edit]).unwrap_err();
    assert!(!host.is_active());
}

#[test]
fn fuel_recovery_reverts_native_edits_when_reinitializing_controls() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut instance = fuel_guest(
        &engine,
        ModGrants {
            controls: true,
            hud: true,
            ..Default::default()
        },
    );
    let panel = serde_json::json!({
        "surface": crate::tests::panel_surface("text"),
        "toggle_key": "ShiftRight",
        "controls": [{"kind":"text","id":"name","label":"Name","value":"Main",
            "min_chars":1,"max_chars":8,"live":true}]
    });
    let bundle = ui::mod_panel::PanelBundle::parse(
        &serde_json::json!({"panel":panel,"surfaces":{}}).to_string(),
    )
    .unwrap();
    instance
        .set_panel_bundle(std::sync::Arc::new(bundle))
        .unwrap();
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
    assert!(instance.recover_frame_fuel().unwrap());
    assert!(
        matches!(&instance.panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == "Main")
    );
    let state = instance.store.data_mut();
    state.controls.begin_text_frame();
    assert!(state.read_text_events().unwrap().unwrap().is_empty());
    assert!(state.read_field_events().unwrap().unwrap().is_empty());
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
fn fuel_recovery_cancels_old_taps_and_keeps_trusted_drawing_observations() {
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
            anchor: cinnabar::extension::touch::Anchor::TopRight,
            offset: cinnabar::extension::touch::Point { x: 4., y: 24. },
            size: cinnabar::extension::touch::Point { x: 36., y: 18. },
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
    instance.store.data_mut().controls.frame.keys_pressed = vec!["KeyQ".into()];
    instance.store.data_mut().controls.frame.keys_held = vec!["KeyQ".into()];
    assert!(instance.queue_touch_press("menu"));
    instance.frame(CallbackOutcome::Executed).unwrap_err();
    assert!(instance.queue_touch_press("menu"));
    instance.recover_frame_fuel().unwrap();
    assert_ne!(instance.touch_owner(), owner);
    {
        let state = instance.store.data_mut();
        state.touch.begin_frame();
        assert!(state.read_presses().unwrap().unwrap().is_empty());
        state.touch.finish_frame(false);
    }
    assert!(instance.queue_touch_press("menu"));
    let state = instance.store.data_mut();
    assert!(state.controls.frame.keys_pressed.is_empty());
    assert!(state.controls.frame.keys_held.is_empty());
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

#[test]
fn fuel_recovery_keeps_presentation_and_discards_failed_parameter_writes() {
    use cinnabar::extension::{presentation::Host as _, render::Rgba};
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut instance = fuel_guest(
        &engine,
        ModGrants {
            hud: true,
            ..Default::default()
        },
    );
    instance
        .store
        .data_mut()
        .set_hurt_overlay(
            Some(Rgba {
                r: 1.,
                g: 0.,
                b: 0.,
                a: 1.,
            }),
            None,
        )
        .unwrap()
        .unwrap();
    commit(&mut instance.store);
    let (published, revision) = instance.store.data().presentation.output();
    let published = published.clone();
    instance
        .run(FRAME_FUEL, commit, |_, store| {
            store.data_mut().set_hurt_overlay(None, None)?.unwrap();
            Err(wasmtime::Trap::OutOfFuel.into())
        })
        .unwrap_err();
    assert_eq!(
        instance.store.data().presentation.output(),
        (&published, revision)
    );
    instance.recover_frame_fuel().unwrap();
    assert_eq!(
        instance.store.data().presentation.output(),
        (&published, revision)
    );
}

#[test]
fn fuel_recovery_seeds_the_initial_panel_before_sparse_init() {
    use cinnabar::extension::panel::Host as _;
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let bundle: ui::mod_panel::PanelBundle = serde_json::from_value(serde_json::json!({
        "panel": {"surface": crate::tests::panel_surface("initial"), "toggle_key":"F10",
            "controls":[{"kind":"toggle","id":"initial","label":"Initial","value":false}]},
        "surfaces":{"other":crate::tests::panel_surface("other")},
        "control_schemas":{"other":[{"kind":"toggle","id":"other","label":"Other","value":false}]}
    }))
    .unwrap();
    bundle.validate().unwrap();
    let patch = r#"{"controls":[{"id":"initial","value":true}]}"#;
    let mut instance = Instance::new(
        &engine,
        crate::tests::recovery_edit_guest(patch, true).as_bytes(),
        ModGrants {
            controls: true,
            settings: true,
            ..Default::default()
        },
        String::new(),
        Declared::default(),
        Some(std::sync::Arc::new(bundle)),
    )
    .unwrap();
    instance
        .store
        .data_mut()
        .update_content(r#"{"surface":"other"}"#.into())
        .unwrap()
        .unwrap();
    commit(&mut instance.store);
    assert_eq!(instance.panel().unwrap().controls[0].id(), "other");
    instance.store.data_mut().pressed = true;
    instance.frame(CallbackOutcome::Executed).unwrap_err();
    assert!(instance.is_active());
    assert!(instance.recover_frame_fuel().unwrap());
    assert!(matches!(&instance.panel().unwrap().controls[0],
        ui::mod_panel::Control::Toggle { id, value: true, .. } if id == "initial"));
    instance.frame(CallbackOutcome::Executed).unwrap();
    assert!(instance.is_active());
}
