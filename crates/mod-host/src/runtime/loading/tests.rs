use super::*;
use mod_api::drawing::{DrawingPermissions, DrawingSession};

const QUIET_COMPONENT: &str = r#"(component
    (core module $m (func (export "init")) (func (export "frame")))
    (core instance $i (instantiate $m))
    (func (export "init") (canon lift (core func $i "init")))
    (func (export "frame") (canon lift (core func $i "frame"))))"#;

/// Admits a component directly into a connected world without replacing its guest.
fn host(source: &str, grants: ModGrants) -> crate::ModHost {
    crate::ModHost::prepare_snapshot_in_session(
        std::path::Path::new("world-loading.wat"),
        source.as_bytes(),
        grants,
        None,
        Some(session()),
    )
    .unwrap()
}

/// The connection remains stable while the destination dimension changes.
fn session() -> DrawingSession {
    DrawingSession {
        connection: 1,
        permissions: DrawingPermissions::ALL,
        ..Default::default()
    }
}

#[test]
fn loading_gap_suppresses_repeated_no_hud_world_output() {
    let package = include_str!("../../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let source = include_str!("../../tests/gameplay.wat")
        .replace("$GAMEPLAY", &format!("{name}/gameplay@{version}"))
        .replace("$INIT", "")
        .replace("$FRAME", "i32.const 1000 i32.const 256 call $delay i32.const 1 i32.const 1200 i32.const 256 call $time")
        .replacen("(component", &format!(r#"(component
            (import "{name}/environment@{version}" (instance $environment
                (export "set-time-override" (func (param "ticks" (option u32))
                    (result (result (error string)))))))
            (alias export $environment "set-time-override" (func $time))"#), 1)
        .replace("(core module $code", "(core func $lower-time (canon lower (func $time) (memory $memory) (realloc $realloc)))\n(core module $code\n(import \"host\" \"time\" (func $time (param i32 i32 i32)))")
        .replace("(core instance $host", "(core instance $host (export \"time\" (func $lower-time))");
    let mut host = host(
        &source,
        ModGrants {
            packet_delay: true,
            environment: true,
            ..Default::default()
        },
    );
    host.frame(false).unwrap();
    assert_eq!(host.packet_delay_ms(), 1000);
    assert_eq!(host.time_override(), Some(1200));
    host.bind_drawing_world(session(), false);
    for _ in 0..3 {
        host.frame(false).unwrap();
        assert!(host.is_active());
        assert_eq!(host.packet_delay_ms(), 0);
        assert_eq!(host.time_override(), None);
    }
    host.bind_drawing_session(DrawingSession {
        dimension: 1,
        ..session()
    });
    assert_eq!(host.packet_delay_ms(), 0);
    assert_eq!(host.time_override(), None);
    host.frame(false).unwrap();
    assert_eq!(host.packet_delay_ms(), 1000);
    assert_eq!(host.time_override(), Some(1200));
}

#[test]
fn loading_gap_clears_actor_presentation_before_deferred_destination_callbacks() {
    use cinnabar::extension::{presentation as wit, presentation::Host as _, render::Rgba};
    for gap in [false, true] {
        let mut host = host(
            QUIET_COMPONENT,
            ModGrants {
                hud: true,
                players: true,
                ..Default::default()
            },
        );
        host.instance
            .run(CALLBACK_FUEL, commit_event, |_, store| {
                let state = store.data_mut();
                let white = Rgba {
                    r: 1.,
                    g: 1.,
                    b: 1.,
                    a: 1.,
                };
                state
                    .set_actor_tints(vec![wit::ActorTint {
                        runtime_id: 42,
                        color: white,
                    }])?
                    .unwrap();
                state
                    .set_annotations(vec![wit::Annotation {
                        runtime_id: 42,
                        nametag: vec![wit::TextSpan {
                            text: "old world".into(),
                            color: white,
                        }],
                        nametag_position: wit::AnnotationPosition::After,
                        roster: Vec::new(),
                        roster_position: wit::AnnotationPosition::Before,
                    }])?
                    .unwrap();
                state.set_hurt_overlay(Some(white), None)?.unwrap();
                Ok(())
            })
            .unwrap();
        let before = host.instance.store.data().presentation.output().1;
        let budget = crate::CallbackBudget::new(0);
        host.set_frame_authority(budget, false, false, 0);
        if gap {
            host.bind_drawing_world(session(), false);
            assert!(
                host.instance
                    .store
                    .data()
                    .presentation
                    .output()
                    .0
                    .is_empty()
            );
            assert_ne!(host.instance.store.data().presentation.output().1, before);
        }
        host.bind_drawing_session(DrawingSession {
            dimension: 1,
            ..session()
        });
        host.frame(false).unwrap();
        let (output, revision) = host.instance.store.data().presentation.output();
        assert!(host.is_active());
        assert!(output.tints.is_empty());
        assert!(output.annotations.is_empty());
        assert!(output.is_empty());
        assert_ne!(revision, before);
    }
}

/// Stages each non-drawing world producer through the same imports used by guests.
fn stage_world_output(state: &mut State) -> Result<()> {
    use cinnabar::extension::{
        camera::{self, Host as _},
        environment::{Host as _, Weather},
        events::Host as _,
        gameplay::Host as _,
        inventory::Host as _,
        item_use::Host as _,
        presentation::{self, Host as _},
        render::Rgba,
    };
    state.snapshot = Some(GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: crate::GameplayVector3 {
            x: 0.,
            y: 64.,
            z: 0.,
        },
        yaw: 0.,
        pitch: 0.,
        attack_held: true,
        frame_seconds: 0.016,
        players: Vec::new(),
    });
    state.gameplay_facts.use_held = true;
    state.controls.frame.focused = true;
    state.controls.frame.gameplay = true;
    state.set_packet_delay(1000)?.unwrap();
    state.set_time_override(Some(1200))?.unwrap();
    state.set_weather_override(Some(Weather::Thunder))?.unwrap();
    state.set_fullbright(true)?.unwrap();
    state
        .set_movement_intents(Some(crate::GameplayMovementIntents {
            sprint: true,
            sneak: true,
        }))?
        .unwrap();
    state
        .set_control_modes(Some(crate::GameplayControlModes {
            toggle_sprint: true,
            toggle_sneak: true,
            always_sprint: true,
        }))?
        .unwrap();
    state.cancel_jump()?.unwrap();
    state.rotate(0.1, 0.1)?.unwrap();
    state
        .set_camera_rig(Some(GameplayCameraRig {
            offset: crate::GameplayVector3 {
                x: 0.,
                y: 0.,
                z: 1.,
            },
            roll: 0.,
            fov_delta: 1.,
        }))?
        .unwrap();
    state.set_preserve_teleport_rotation(true)?.unwrap();
    state.set_view_scale(0.8, 0.8)?.unwrap();
    state
        .set_view_tilt(Some(camera::ViewTilt {
            roll: 1.,
            pitch: 1.,
            suppress_hurt_shake: true,
        }))?
        .unwrap();
    state.set_attack_reach(Some(4.))?.unwrap();
    state.pulse_attack()?.unwrap();
    state.pulse_use()?.unwrap();
    state.fill_recipe(7)?.unwrap();
    state.request_command("/fixture".into())?.unwrap();
    state.emit("fixture".into(), vec![1.])?.unwrap();
    state
        .set_actor_tints(vec![presentation::ActorTint {
            runtime_id: 42,
            color: Rgba {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 1.,
            },
        }])?
        .unwrap();
    Ok(())
}

/// Every world output is empty even when the caller supplied stale gameplay input.
fn assert_no_world_output(host: &mut crate::ModHost) {
    assert_eq!(host.packet_delay_ms(), 0);
    assert_eq!(host.time_override(), None);
    assert!(host.weather_override().is_none());
    assert!(!host.fullbright());
    assert!(host.movement_intents().is_none());
    assert!(host.control_modes().is_none());
    assert!(!host.take_jump_cancel());
    assert!(!host.take_jump_pulse());
    assert!(host.take_camera_delta().is_none());
    assert!(host.camera_rig().is_none());
    assert!(!host.preserves_teleport_rotation());
    assert!(host.camera_view_scale().is_none());
    assert!(host.camera_view_tilt().is_none());
    assert_eq!(host.take_interaction(), crate::InteractionOutput::default());
    assert!(!host.use_pulse_requested());
    assert!(host.take_inventory_requests().is_empty());
    assert!(host.take_commands().is_empty());
    assert!(host.take_cues().is_empty());
    assert!(
        host.instance
            .store
            .data()
            .presentation
            .output()
            .0
            .is_empty()
    );
    assert!(host.render().0.retained.is_empty());
    assert!(host.render().0.primitives.is_empty());
    assert!(host.render().0.passes.is_empty());
    assert!(host.render().0.masks.is_empty());
}

#[test]
fn loading_gap_fences_all_world_producers_for_frames_and_events() {
    let mut host = host(
        QUIET_COMPONENT,
        ModGrants {
            hud: true,
            players: true,
            packet_delay: true,
            environment: true,
            fullbright: true,
            camera: true,
            item_use: true,
            movement: true,
            interaction: true,
            inventory: true,
            commands: vec!["fixture".into()],
            ..Default::default()
        },
    );
    host.instance
        .run(FRAME_FUEL, commit, |_, store| {
            stage_world_output(store.data_mut())
        })
        .unwrap();
    assert_eq!(host.packet_delay_ms(), 1000);
    assert!(host.attack_pulse_requested());
    assert!(host.use_pulse_requested());
    assert!(host.camera_view_tilt().is_some());
    host.bind_drawing_world(session(), false);
    assert_no_world_output(&mut host);
    for fuel in [FRAME_FUEL, CALLBACK_FUEL, LOAD_FUEL] {
        host.instance.begin_frame();
        host.instance
            .run(
                fuel,
                if fuel == FRAME_FUEL {
                    commit
                } else {
                    commit_event
                },
                |_, store| stage_world_output(store.data_mut()),
            )
            .unwrap();
        assert_no_world_output(&mut host);
    }
    host.bind_drawing_session(DrawingSession {
        dimension: 1,
        ..session()
    });
    assert_no_world_output(&mut host);
}

#[test]
fn loading_gap_invalidates_world_projections_but_keeps_local_hud_and_panel_content() {
    use cinnabar::extension::panel::Host as _;
    let mut host = host(
        QUIET_COMPONENT,
        ModGrants {
            hud: true,
            players: true,
            controls: true,
            ..Default::default()
        },
    );
    let surface = serde_json::json!({
        "screen": "fixture.root",
        "document": r#"{"namespace":"fixture","root":{"type":"panel"}}"#,
        "bindings": {
            "#label": "local HUD",
            "#actor": {"project": {"actor": 42}},
            "#point": {"project": {"world": [1., 64., 2.]}}
        }
    });
    let publish = |state: &mut State| -> Result<()> {
        hud::surface::set(state, surface.to_string())?.unwrap();
        state
            .set_content(
                serde_json::json!({
                    "surface": surface, "toggle_key": "ShiftRight", "controls": []
                })
                .to_string(),
            )?
            .unwrap();
        Ok(())
    };
    host.instance
        .run(CALLBACK_FUEL, commit_event, |_, store| {
            publish(store.data_mut())
        })
        .unwrap();
    let (previous, revision) = host.hud_surface().map(|(s, r)| (Arc::clone(s), r)).unwrap();
    assert_eq!(previous.bindings.len(), 3);
    host.bind_drawing_world(session(), false);
    for republish in [false, true] {
        if republish {
            host.instance
                .run(CALLBACK_FUEL, commit_event, |_, store| {
                    publish(store.data_mut())
                })
                .unwrap();
        }
        let (current, next) = host.hud_surface().unwrap();
        assert_eq!(current.document, previous.document);
        assert_ne!(next, revision);
        for surface in [current.as_ref(), &host.panel().unwrap().surface] {
            assert_eq!(surface.bindings.len(), 3);
            for name in ["#actor", "#point"] {
                assert!(surface.bindings[name].projection().is_some());
                assert!(surface.bindings[name].active_projection().is_none());
            }
            assert_eq!(
                surface.bindings["#label"],
                ui::mod_panel::SurfaceValue::Text("local HUD".into())
            );
        }
    }
    host.bind_drawing_session(DrawingSession {
        dimension: 1,
        ..session()
    });
    assert!(
        host.hud_surface().unwrap().0.bindings["#point"]
            .active_projection()
            .is_none()
    );
    host.instance
        .run(CALLBACK_FUEL, commit_event, |_, store| {
            publish(store.data_mut())
        })
        .unwrap();
    assert_eq!(host.hud_surface().unwrap().0.bindings.len(), 3);
    assert!(
        host.hud_surface().unwrap().0.bindings["#point"]
            .active_projection()
            .is_some()
    );
    assert!(host.is_active());
}
