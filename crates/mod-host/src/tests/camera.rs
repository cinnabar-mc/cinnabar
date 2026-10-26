use super::*;

fn source(frame: &str) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("camera.wat")
        .replace("$CAMERA", &format!("{name}/camera@{version}"))
        .replace("$FRAME", frame)
}

fn load(frame: &str, camera: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("camera.wat");
    std::fs::write(&path, source(frame)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            camera,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x: 0.0,
            y: 64.0,
            z: 0.0,
        },
        yaw: 0.0,
        pitch: 0.0,
        attack_held: false,
        frame_seconds: 0.016,
        players: Vec::new(),
    }
}

fn call(enabled: bool, denied: bool) -> String {
    format!(
        "i32.const {} i32.const 256 call $preserve i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(enabled),
        u8::from(denied)
    )
}

#[test]
fn camera_policy_is_granted_per_gameplay_frame_and_false_disables_it() {
    let (_dir, mut denied) = load(&call(true, true), false);
    denied.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(!denied.preserves_teleport_rotation());
    let frame = format!(
        "global.get $count i32.const 1 i32.eq if {} else {} end",
        call(true, false),
        call(false, false)
    );
    let (_dir, mut host) = load(&frame, true);
    assert!(!host.preserves_teleport_rotation());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(host.preserves_teleport_rotation());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(!host.preserves_teleport_rotation());
    let (_dir, mut menus) = load(&call(true, false), true);
    menus.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(menus.preserves_teleport_rotation());
    menus.frame(false).unwrap();
    assert!(!menus.preserves_teleport_rotation());
}

#[test]
fn omission_traps_and_reload_revoke_camera_policy() {
    let first = format!(
        "global.get $count i32.const 1 i32.eq if {} end",
        call(true, false)
    );
    let (_dir, mut omitted) = load(&first, true);
    omitted
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(omitted.preserves_teleport_rotation());
    omitted
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(!omitted.preserves_teleport_rotation());
    let frame = format!(
        "{} global.get $count i32.const 2 i32.eq if unreachable end",
        call(true, false)
    );
    let (_dir, mut trapped) = load(&frame, true);
    trapped
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(trapped.preserves_teleport_rotation());
    assert!(
        trapped
            .frame_with_gameplay(false, Some(snapshot()))
            .is_err()
    );
    assert!(!trapped.is_active());
    assert!(!trapped.preserves_teleport_rotation());
    trapped
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(!trapped.preserves_teleport_rotation());
    let (dir, mut reloaded) = load(&call(true, false), true);
    reloaded
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(reloaded.preserves_teleport_rotation());
    std::fs::write(dir.path().join("camera.wat"), source("")).unwrap();
    assert!(reloaded.reload_if_changed().unwrap());
    assert!(!reloaded.preserves_teleport_rotation());
}

#[test]
fn camera_import_budget_quarantines_excess_writes() {
    let calls = std::iter::repeat_n(call(true, false), 9)
        .collect::<Vec<_>>()
        .join(" ");
    let (_dir, mut host) = load(&calls, true);
    assert!(host.frame_with_gameplay(false, Some(snapshot())).is_err());
    assert!(!host.is_active());
    assert!(!host.preserves_teleport_rotation());
}

/// Calls the view import and checks the canonical result's success/error tag.
fn view_call(fov: &str, look: &str, denied: bool) -> String {
    format!(
        "f32.const {fov} f32.const {look} i32.const 256 call $view-scale i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(denied)
    )
}

/// Supplies a focused, input-owned frame without requiring the input-read grant.
fn view_frame(
    host: &mut ModHost,
    focused: bool,
    panel_open: bool,
    gameplay: bool,
) -> anyhow::Result<()> {
    host.frame_with_controls(
        false,
        gameplay.then(snapshot),
        ControlFrame {
            focused,
            gameplay,
            panel_open,
            ..empty_controls()
        },
    )
}

/// Calls the actual lowered option-record import and checks its success or refusal.
fn tilt_call(present: bool, roll: &str, refused: bool) -> String {
    format!(
        "i32.const {} f32.const {roll} f32.const 0 i32.const 1 i32.const 256 call $view-tilt i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(present),
        u8::from(refused)
    )
}

fn tilt(roll: f32) -> mod_api::native_camera::ViewTilt {
    mod_api::native_camera::ViewTilt {
        roll,
        pitch: 0.0,
        suppress_hurt_shake: true,
    }
}

/// A tilt needs the camera grant and finite degrees within its bound.
#[test]
fn view_tilt_requires_the_camera_grant_and_bounded_degrees() {
    let (_dir, mut denied) = load(&tilt_call(true, "14", true), false);
    denied.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(denied.is_active());
    assert_eq!(denied.camera_view_tilt(), None);
    assert!(!denied.has_camera_output());
    let bound = mod_api::MAX_VIEW_TILT_DEGREES;
    for roll in ["nan", "inf", &(bound + 1.0).to_string()] {
        let (_dir, mut host) = load(&tilt_call(true, roll, true), true);
        host.frame_with_gameplay(false, Some(snapshot())).unwrap();
        assert_eq!(host.camera_view_tilt(), None, "{roll}");
    }
    let (_dir, mut host) = load(&tilt_call(true, &(-bound).to_string(), false), true);
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(host.camera_view_tilt(), Some(tilt(-bound)));
}

/// A tilt lasts the frame that set it, applies while a menu owns input, and ends on omission,
/// none, a trap or reload.
#[test]
fn view_tilt_is_current_frame_only_and_revoked_with_the_guest() {
    let frame = format!(
        "global.get $count i32.const 2 i32.ne if {} end global.get $count i32.const 3 i32.eq if {} end",
        tilt_call(true, "14", false),
        tilt_call(false, "0", false)
    );
    let (_dir, mut host) = load(&frame, true);
    host.frame(false).unwrap();
    assert_eq!(
        host.camera_view_tilt(),
        Some(tilt(14.0)),
        "menus keep the tilt"
    );
    assert!(host.has_camera_output());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(host.camera_view_tilt(), None, "omission restores the view");
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(host.camera_view_tilt(), None, "none restores the view");
    let trapping = format!(
        "{} global.get $count i32.const 2 i32.eq if unreachable end",
        tilt_call(true, "14", false)
    );
    let (_dir, mut trapped) = load(&trapping, true);
    trapped.frame(false).unwrap();
    assert!(trapped.frame(false).is_err());
    assert_eq!(trapped.camera_view_tilt(), None);
    let (dir, mut reloaded) = load(&tilt_call(true, "14", false), true);
    reloaded.frame(false).unwrap();
    assert!(reloaded.camera_view_tilt().is_some());
    std::fs::write(dir.path().join("camera.wat"), source("")).unwrap();
    assert!(reloaded.reload_if_changed().unwrap());
    assert_eq!(reloaded.camera_view_tilt(), None);
}

#[test]
fn view_scale_validates_finite_bounds_and_requires_camera_grant() {
    let (_dir, mut denied) = load(&view_call("0.25", "0.25", true), false);
    view_frame(&mut denied, true, false, true).unwrap();
    assert_eq!(denied.camera_view_scale(), None);
    for (fov, look) in [
        ("0.09", "0.25"),
        ("1.1", "0.25"),
        ("nan", "0.25"),
        ("inf", "0.25"),
        ("0.25", "0.04"),
        ("0.25", "1.1"),
        ("0.25", "nan"),
    ] {
        let (_dir, mut host) = load(&view_call(fov, look, true), true);
        view_frame(&mut host, true, false, true).unwrap();
        assert_eq!(host.camera_view_scale(), None);
    }
    let (_dir, mut host) = load(&view_call("0.1", "0.05", false), true);
    view_frame(&mut host, true, false, true).unwrap();
    assert_eq!(
        host.camera_view_scale(),
        Some([mod_api::MIN_VIEW_FOV_SCALE, mod_api::MIN_VIEW_LOOK_SCALE])
    );
}

#[test]
fn view_scale_expires_on_omission_neutral_focus_ui_and_missing_gameplay() {
    let frame = format!(
        "global.get $count i32.const 2 i32.ne if {} end global.get $count i32.const 4 i32.eq if {} end",
        view_call("0.25", "0.25", false),
        view_call("1", "1", false)
    );
    let (_dir, mut host) = load(&frame, true);
    assert_eq!(host.camera_view_scale(), None);
    for expected in [Some([0.25; 2]), None, Some([0.25; 2]), None] {
        view_frame(&mut host, true, false, true).unwrap();
        assert_eq!(host.camera_view_scale(), expected);
    }
    for (focused, panel_open, gameplay) in [
        (false, false, true),
        (true, true, true),
        (true, false, false),
    ] {
        view_frame(&mut host, focused, panel_open, gameplay).unwrap();
        assert_eq!(host.camera_view_scale(), None);
    }
}

#[test]
fn view_scale_trap_reload_and_shared_import_budget_revoke_output() {
    let frame = format!(
        "{} global.get $count i32.const 2 i32.eq if unreachable end",
        view_call("0.25", "0.25", false)
    );
    let (_dir, mut host) = load(&frame, true);
    view_frame(&mut host, true, false, true).unwrap();
    assert_eq!(host.camera_view_scale(), Some([0.25; 2]));
    assert!(view_frame(&mut host, true, false, true).is_err());
    assert_eq!(host.camera_view_scale(), None);
    let (dir, mut host) = load(&view_call("0.25", "0.25", false), true);
    view_frame(&mut host, true, false, true).unwrap();
    std::fs::write(dir.path().join("camera.wat"), source("")).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_eq!(host.camera_view_scale(), None);
    let calls = format!(
        "{} {}",
        view_call("0.25", "0.25", false),
        std::iter::repeat_n(call(true, false), 8)
            .collect::<Vec<_>>()
            .join(" ")
    );
    let (_dir, mut host) = load(&calls, true);
    assert!(view_frame(&mut host, true, false, true).is_err());
    assert_eq!(host.camera_view_scale(), None);
}

#[test]
fn deferred_lifecycle_and_frame_callbacks_cannot_republish_camera_policies() {
    let frame = format!("{} {}", call(true, false), view_call("0.25", "0.25", false));
    let (_dir, mut host) = super::load_with_view_closed(
        source(&frame),
        ModGrants {
            camera: true,
            ..Default::default()
        },
        "",
    );
    view_frame(&mut host, true, false, true).unwrap();
    assert!(host.preserves_teleport_rotation());
    assert_eq!(host.camera_view_scale(), Some([0.25; 2]));
    let budget = super::defer_view_closed(&mut host);
    view_frame(&mut host, false, false, false).unwrap();
    assert!(host.is_active());
    assert!(!host.preserves_teleport_rotation());
    assert!(!host.has_camera_output());
    assert_eq!(host.camera_view_scale(), None);
    budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    view_frame(&mut host, true, false, true).unwrap();
    assert_eq!(host.camera_view_scale(), Some([0.25; 2]));
    budget.reset(0);
    view_frame(&mut host, true, false, true).unwrap();
    assert!(!host.preserves_teleport_rotation());
    assert!(!host.has_camera_output());
    assert_eq!(host.camera_view_scale(), None);
}

#[test]
fn lifecycle_callbacks_cannot_publish_camera_policy_through_the_next_frame() {
    for deferred in [false, true] {
        let event = format!(
            "{} {} i32.const 7 global.set $count",
            call(true, false),
            view_call("0.25", "0.25", false)
        );
        let (_dir, mut host) = super::load_with_view_closed(
            source("global.get $count i32.const 8 i32.ne if unreachable end"),
            ModGrants {
                camera: true,
                ..Default::default()
            },
            &event,
        );
        if deferred {
            let budget = super::defer_view_closed(&mut host);
            budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        } else {
            assert_eq!(
                host.dispatch(vec![ModEvent::ViewClosed]).unwrap(),
                CallbackOutcome::Executed
            );
        }
        view_frame(&mut host, true, false, true).unwrap();
        assert!(host.is_active());
        assert!(!host.preserves_teleport_rotation());
        assert_eq!(host.camera_view_scale(), None);
        assert!(!host.has_camera_output());
    }
}
