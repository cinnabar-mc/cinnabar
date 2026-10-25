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
    host.set_panel_bundle(r#"{"panel":{"title":"Retained","toggle_key":"ShiftRight","dark":true,"controls":[]},"surfaces":{}}"#).unwrap();
    host.set_panel_open(true);
    (directory, host)
}

#[test]
fn frame_fuel_overrun_retains_output_and_recovers_a_fresh_guest() {
    let (_directory, mut host) = host();
    let panel = host.panel().unwrap().clone();
    host.frame(true).unwrap_err();
    assert!(host.is_active());
    assert_eq!(host.label(), Some("Hello"));
    assert_eq!(host.panel(), Some(&panel));
    assert!(host.panel_open());
    assert_eq!(host.last_fuel_used(), FRAME_FUEL);
    host.frame(false).unwrap();
    assert!(host.is_active());
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
    host.set_frame_authority(budget.clone(), false, false, 0, Default::default());
    for _ in 0..crate::runtime::MAX_CONSECUTIVE_FRAME_OVERRUNS {
        host.frame(true).unwrap_err();
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
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
    host.set_frame_authority(budget.clone(), false, false, 0, Default::default());
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
        .replace("(func (export \"frame\")", "(func (export \"frame\") call $pressed if i32.const 1024 i32.const 2 i32.const 512 call $save (loop $forever br $forever) end");
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
    host.set_frame_authority(CallbackBudget::new(0), false, false, 0, Default::default());
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert_eq!(host.settings_snapshot(), Some(settings));
}

#[test]
fn frame_fuel_overrun_keeps_the_last_published_hud_and_crosshair() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hud.wat");
    let source = hud::source(
        r#"{"cards":[{"id":"retained","rows":[]}]}"#,
        r#"{"shape":"circle","color":[0.3,1,0.5,1]}"#,
        "call $cards call $cursor",
        "call $clear (loop $forever br $forever)",
        false,
    );
    std::fs::write(&path, source).unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            hud: true,
            ..Default::default()
        },
    )
    .unwrap();
    let hud = host.hud().unwrap().clone();
    let revision = host.hud_revision();
    let crosshair = host.crosshair().unwrap().clone();
    host.frame(false).unwrap_err();
    assert!(host.is_active());
    assert_eq!(host.hud(), Some(&hud));
    assert_eq!(host.hud_revision(), revision);
    assert_eq!(host.crosshair(), Some(&crosshair));
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
        .replace("(func (export \"init\")", "(func (export \"init\") i32.const 800 call $load i32.const 808 i32.load if unreachable end")
        .replace("(func (export \"frame\")", "(func (export \"frame\") (loop $forever br $forever)");
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

#[test]
fn frame_fuel_overrun_releases_only_uncommitted_hud_claims() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("claims.wat");
    let content = r#"{"cards":[{"id":"old","rows":[]}]}"#;
    let offset = content.find("old").unwrap();
    let frame = format!(
        "i32.const {offset} i32.const {} i32.store8 call $cards (loop $forever br $forever)",
        b'n'
    );
    std::fs::write(
        &path,
        hud::source(content, "", "call $cards", &frame, false),
    )
    .unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            hud: true,
            ..Default::default()
        },
    )
    .unwrap();
    let claims = HudClaims::default();
    host.set_frame_authority(
        CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL),
        false,
        false,
        0,
        claims.clone(),
    );
    host.frame(false).unwrap_err();
    assert!(host.is_active());
    assert_eq!(host.hud().unwrap().cards[0].id, "old");
    assert!(claims.claim(1, std::iter::once("nld".to_owned())));
    assert!(!claims.claim(1, std::iter::once("old".to_owned())));
}
