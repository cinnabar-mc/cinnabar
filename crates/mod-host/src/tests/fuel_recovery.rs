use super::*;

/// Loads a guest that stages a different label before overrunning on pressed frames.
fn host() -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("overrun.wat");
    let source = fixture(
        "call $pressed if
            i32.const 1 i32.const 4 i32.const 128 call $label
            (loop $forever br $forever)
         end",
        "Hello",
    );
    std::fs::write(&path, source).unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            controls: true,
            ..Default::default()
        },
    )
    .unwrap();
    let bundle = serde_json::json!({
        "panel": {"surface": panel_surface("retained"),
            "toggle_key": "ShiftRight", "controls": []},
        "surfaces": {}
    });
    host.set_panel_bundle(&bundle.to_string()).unwrap();
    host.set_panel_open(true);
    (directory, host)
}

#[test]
fn frame_fuel_overrun_retains_output_and_recovers_a_fresh_guest() {
    let (_directory, mut host) = host();
    let panel = host.panel().unwrap().clone();
    host.frame(true).unwrap_err();
    assert!(host.is_active());
    assert!(host.is_recovering());
    assert_eq!(host.label(), Some("Hello"));
    assert_eq!(host.panel(), Some(&panel));
    assert!(host.panel_open());
    assert_eq!(host.last_fuel_used(), FRAME_FUEL);
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert!(!host.is_recovering());
    assert_eq!(host.label(), Some("Hello"));
    assert_eq!(host.panel(), Some(&panel));
    assert!(host.panel_open());
}

#[test]
fn repeated_frame_fuel_overruns_quarantine_the_guest() {
    let (_directory, mut host) = host();
    // The first overrun must be recoverable even when the guest will overrun again.
    host.frame(true).unwrap_err();
    assert!(host.is_active());
    for _ in 1..crate::runtime::MAX_CONSECUTIVE_FRAME_OVERRUNS {
        host.frame(true).unwrap();
        host.frame(true).unwrap_err();
    }
    assert!(!host.is_active());
    assert!(host.label().is_none());
    assert!(host.panel().is_none());
    host.frame(false).unwrap();
    assert!(!host.is_active());
}

#[test]
fn successful_frames_reset_overruns_but_deferred_frames_do_not() {
    let (_directory, mut host) = host();
    let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    host.set_frame_authority(budget.clone(), false, false, 0);
    for _ in 0..crate::runtime::MAX_CONSECUTIVE_FRAME_OVERRUNS {
        host.frame(true).unwrap_err();
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        host.frame(false).unwrap();
        host.frame(false).unwrap();
        assert!(host.is_active());
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    }
    for index in 0..crate::runtime::MAX_CONSECUTIVE_FRAME_OVERRUNS {
        host.frame(true).unwrap_err();
        budget.reset(0);
        host.frame(false).unwrap();
        assert_eq!(
            host.is_active(),
            index + 1 < crate::runtime::MAX_CONSECUTIVE_FRAME_OVERRUNS
        );
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    }
    assert!(!host.is_active());
}

#[test]
fn busy_loading_lane_defers_recovery_without_reentering_the_trapped_guest() {
    let (_directory, mut host) = host();
    let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    host.set_frame_authority(budget.clone(), false, false, 0);
    host.frame(true).unwrap_err();
    assert_eq!(
        budget.remaining(),
        mod_api::TOTAL_MOD_FRAME_FUEL - FRAME_FUEL
    );
    assert!(
        budget
            .reserve(server_experience::runtime::LOAD_FUEL)
            .is_some()
    );
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert_eq!(host.label(), Some("Hello"));
    assert!(host.panel_open());
    budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    host.frame(false).unwrap();
    assert!(host.is_active());
}

#[test]
fn fuel_recovery_preserves_committed_settings_and_rejects_failed_writes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.wat");
    let settings = r#"{"done":true}"#;
    let source = prepared_settings::settings_guest("{}", settings)
        .replace("i32.const 1024 i32.const 2 i32.const 512 call $save", "")
        .replacen("(func (export \"frame\")", "(func (export \"frame\") call $pressed if i32.const 1024 i32.const 2 i32.const 512 call $save (loop $forever br $forever) end", 1);
    std::fs::write(&path, source).unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            settings: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.frame(false).unwrap();
    assert_eq!(host.settings_snapshot(), Some(settings));
    host.frame(true).unwrap_err();
    assert_eq!(host.settings_snapshot(), Some(settings));
    // Successful init of a fresh store must retain the committed in-memory preferences.
    host.set_frame_authority(CallbackBudget::new(0), false, false, 0);
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert_eq!(host.settings_snapshot(), Some(settings));
}

#[test]
fn authority_revocation_cancels_pending_fuel_recovery() {
    let (_directory, mut host) = host();
    host.frame(true).unwrap_err();
    host.revoke();
    host.frame(false).unwrap();
    assert!(host.is_revoked());
    assert!(!host.is_active());
    assert!(host.label().is_none());
}

#[test]
fn failed_recovery_initialization_quarantines_instead_of_retrying() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("failed-recovery.wat");
    // The first init commits preferences. A fresh init sees them and traps.
    let source = prepared_settings::settings_guest("{}", "{}")
        .replacen("(export \"save\" (func", "(export \"load\" (func (result (result string (error string))))) (export \"save\" (func", 1)
        .replace("(alias export $settings \"save\"", "(alias export $settings \"load\" (func $get-settings)) (alias export $settings \"save\"")
        .replace("(core func $lower-settings", "(core func $lower-load (canon lower (func $get-settings) (memory $memory) (realloc $realloc))) (core func $lower-settings")
        .replace("(import \"host\" \"save\"", "(import \"host\" \"load\" (func $load (param i32))) (import \"host\" \"save\"")
        .replace("(export \"save\" (func $lower-settings))", "(export \"load\" (func $lower-load)) (export \"save\" (func $lower-settings))")
        .replacen("(func (export \"init\")", "(func (export \"init\") i32.const 800 call $load i32.const 808 i32.load if unreachable end", 1)
        .replacen("(func (export \"frame\")", "(func (export \"frame\") (loop $forever br $forever)", 1);
    std::fs::write(&path, source).unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            settings: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.frame(false).unwrap_err();
    assert!(host.is_active());
    let error = host.frame(false).unwrap_err();
    assert!(format!("{error:#}").contains("recovery failed"));
    assert!(!host.is_active());
    assert!(host.label().is_none());
    host.frame(false).unwrap();
}

/// Loads a guest that applies native values before overrunning while the action is pressed.
fn edit_host(live: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("edits.wat");
    let panel = serde_json::json!({"surface": panel_surface("edits"), "toggle_key": "ShiftRight",
        "controls": [{"kind":"text", "id":"value", "label":"Value", "value":r#"{"v":0}"#,
            "min_chars":4,"live":live,"navigation_keys":true},
            {"kind":"keybind", "id":"capture", "label":"Capture", "key":"KeyQ", "capturing":false}]});
    let json = panel.to_string();
    let source = recovery_edit_guest(&json, false);
    std::fs::write(&path, source).unwrap();
    std::fs::write(settings_companion(&path), r#"{"v":0}"#).unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            controls: true,
            settings: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut previous = panel;
    previous["controls"][1]["capturing"] = serde_json::json!(true);
    previous["surface"]["bindings"] = serde_json::json!({"#stale":"old"});
    let bundle = serde_json::json!({"panel": previous, "surfaces": {}});
    host.set_panel_bundle(&bundle.to_string()).unwrap();
    (directory, host)
}

#[test]
fn fuel_recovery_reverts_native_text_queued_before_the_overrun() {
    let (_directory, mut host) = edit_host(true);
    host.queue_text_events(vec![
        ui::mod_panel::TextEvent {
            id: "value".into(),
            value: r#"{"v":1}"#.into(),
        },
        ui::mod_panel::TextEvent {
            id: "value".into(),
            value: r#"{"v":2}"#.into(),
        },
    ])
    .unwrap();
    assert_edit_recovery(&mut host);
}

#[test]
fn fuel_recovery_reverts_field_values_without_replaying_edges() {
    use ui::mod_panel::{FieldEvent, FieldKey, TextEvent};
    let (_directory, mut host) = edit_host(true);
    host.queue_field_events(vec![
        FieldEvent::Change(TextEvent {
            id: "value".into(),
            value: r#"{"v":1}"#.into(),
        }),
        FieldEvent::Submit(TextEvent {
            id: "value".into(),
            value: r#"{"v":2}"#.into(),
        }),
        FieldEvent::Key(FieldKey {
            id: "value".into(),
            key: "ArrowDown".into(),
            modifiers: Default::default(),
        }),
    ])
    .unwrap();
    assert_edit_recovery(&mut host);
}

#[test]
fn fuel_recovery_reverts_non_live_submissions_to_committed_preferences() {
    let (_directory, mut host) = edit_host(false);
    host.queue_field_events(vec![ui::mod_panel::FieldEvent::Submit(
        ui::mod_panel::TextEvent {
            id: "value".into(),
            value: r#"{"v":2}"#.into(),
        },
    )])
    .unwrap();
    assert_edit_recovery(&mut host);
}

#[test]
fn fuel_recovery_rejects_values_below_the_initialized_control_minimum() {
    let (_directory, mut host) = edit_host(false);
    let mut previous = serde_json::to_value(host.panel().unwrap()).unwrap();
    previous["controls"][0]["min_chars"] = serde_json::json!(0);
    host.set_panel_bundle(&serde_json::json!({"panel":previous,"surfaces":{}}).to_string())
        .unwrap();
    host.queue_field_events(vec![ui::mod_panel::FieldEvent::Submit(
        ui::mod_panel::TextEvent {
            id: "value".into(),
            value: "{}".into(),
        },
    )])
    .unwrap();
    host.frame(true).unwrap_err();
    assert!(host.is_active());
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert!(
        matches!(&host.panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == r#"{"v":0}"#)
    );
    assert_eq!(host.label(), Some(r#"{"v":0}"#));
    assert_eq!(host.settings_snapshot(), Some(r#"{"v":0}"#));
}

#[test]
fn fuel_recovery_drops_input_sampled_before_guest_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input-fence.wat");
    std::fs::write(
        &path,
        fixture(
            "call $pressed if (loop $forever br $forever) end
         i32.const 1 i32.const 4 i32.const 128 call $label",
            "Hello",
        ),
    )
    .unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            controls: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.frame(true).unwrap_err();
    host.frame_with_controls(
        false,
        None,
        ControlFrame {
            events: vec![crate::ControlEvent {
                id: "old-route".into(),
                value: 1.,
            }],
            focused: true,
            panel_open: true,
            pointer: Some((4., 8.)),
            wheel: 1.,
            ..empty_controls()
        },
    )
    .unwrap();
    assert!(host.is_active());
    assert_eq!(host.label(), Some("Hello"));
    host.frame(false).unwrap();
    assert_eq!(host.label(), Some("ello"));
}

#[test]
fn fuel_recovery_discards_edits_for_controls_replaced_by_init() {
    let (_directory, mut host) = edit_host(true);
    let mut previous = serde_json::to_value(host.panel().unwrap()).unwrap();
    previous["controls"][0]["id"] = serde_json::json!("removed");
    host.set_panel_bundle(&serde_json::json!({"panel":previous,"surfaces":{}}).to_string())
        .unwrap();
    host.queue_text_events(vec![ui::mod_panel::TextEvent {
        id: "removed".into(),
        value: r#"{"v":2}"#.into(),
    }])
    .unwrap();
    host.frame(true).unwrap_err();
    assert!(host.is_active());
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert_eq!(host.panel().unwrap().controls[0].id(), "value");
    assert!(
        matches!(&host.panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == r#"{"v":0}"#)
    );
    assert_eq!(host.label(), Some(r#"{"v":0}"#));
    assert_eq!(host.settings_snapshot(), Some(r#"{"v":0}"#));
}

/// Checks that rejected edits revert to the initialized panel and committed preferences.
fn assert_edit_recovery(host: &mut ModHost) {
    host.frame(true).unwrap_err();
    assert!(host.is_active());
    assert_eq!(host.label(), Some(r#"{"v":0}"#));
    assert_eq!(host.settings_snapshot(), Some(r#"{"v":0}"#));
    assert!(
        matches!(&host.panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == r#"{"v":2}"#)
    );
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert_eq!(host.label(), Some(r#"{"v":0}"#));
    assert_eq!(host.settings_snapshot(), Some(r#"{"v":0}"#));
    assert!(
        matches!(&host.panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == r#"{"v":0}"#)
    );
    assert!(matches!(
        &host.panel().unwrap().controls[1],
        ui::mod_panel::Control::Keybind {
            capturing: false,
            ..
        }
    ));
    assert!(
        !host
            .panel()
            .unwrap()
            .surface
            .bindings
            .contains_key("#stale")
    );
    host.frame(false).unwrap();
    assert!(host.is_active());
}
