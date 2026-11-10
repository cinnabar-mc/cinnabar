use super::*;

fn source(frame: &str) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("item_use.wat")
        .replace("$ITEM_USE", &format!("{name}/item-use@{version}"))
        .replace("$FRAME", frame)
}

fn load(frame: &str, item_use: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("item_use.wat");
    std::fs::write(&path, source(frame)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            item_use,
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

/// Calls the lowered pulse import and checks its success or refusal.
fn call(refused: bool) -> String {
    format!(
        "i32.const 256 call $pulse i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(refused)
    )
}

/// Calls the lowered pulse import, accepting either result.
const CALL_ANY: &str = "i32.const 256 call $pulse";

fn held() -> GameplayFacts {
    GameplayFacts {
        use_held: true,
        ..Default::default()
    }
}

/// A pulse needs the item-use grant, a gameplay frame and a physically held use control.
#[test]
fn pulse_use_needs_its_grant_a_gameplay_frame_and_a_held_use() {
    let (_dir, mut denied) = load(&call(true), false);
    denied.observe_gameplay(held()).unwrap();
    denied.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(denied.is_active() && !denied.use_pulse_requested());
    let (_dir, mut released) = load(&call(true), true);
    released
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(!released.use_pulse_requested());
    let (_dir, mut menu) = load(&call(true), true);
    menu.observe_gameplay(held()).unwrap();
    menu.frame_with_gameplay(false, None).unwrap();
    assert!(!menu.use_pulse_requested());
    let (_dir, mut granted) = load(&call(false), true);
    granted.observe_gameplay(held()).unwrap();
    granted
        .frame_with_gameplay(false, Some(snapshot()))
        .unwrap();
    assert!(granted.use_pulse_requested());
}

/// A pulse is one edge for its own frame; it is not retained, and a trap publishes none.
#[test]
fn pulse_use_lasts_one_frame_and_a_trap_drops_it() {
    let (_dir, mut host) = load(CALL_ANY, true);
    host.observe_gameplay(held()).unwrap();
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(host.use_pulse_requested());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert!(
        !host.use_pulse_requested(),
        "facts and pulses belong to one frame"
    );
    host.observe_gameplay(held()).unwrap();
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    host.begin_frame();
    assert!(!host.use_pulse_requested());
    let (_dir, mut trapped) = load(&format!("{CALL_ANY} unreachable"), true);
    trapped.observe_gameplay(held()).unwrap();
    assert!(
        trapped
            .frame_with_gameplay(false, Some(snapshot()))
            .is_err()
    );
    assert!(!trapped.use_pulse_requested());
}

#[test]
fn item_use_import_budget_quarantines_excess_writes() {
    let calls = std::iter::repeat_n(call(true), 9)
        .collect::<Vec<_>>()
        .join(" ");
    let (_dir, mut host) = load(&calls, true);
    assert!(host.frame_with_gameplay(false, Some(snapshot())).is_err());
    assert!(!host.is_active());
    assert!(!host.use_pulse_requested());
}
