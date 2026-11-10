use super::*;

fn source(init: &str, frame: &str) -> String {
    source_from(include_str!("player_state.wat"), init, frame)
}

/// Resolves the current interface name while preserving a component fixture's import types.
fn source_from(template: &str, init: &str, frame: &str) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    template
        .replace("$PLAYER_STATE", &format!("{name}/player-state@{version}"))
        .replace("$INIT", init)
        .replace("$FRAME", frame)
}

fn load(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.wat");
    std::fs::write(&path, source(init, frame)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            player_state: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

fn snapshot() -> PlayerStateSnapshot {
    let unknown = PlayerStateSlot {
        known: false,
        item: None,
    };
    PlayerStateSnapshot {
        session: 7,
        dimension: 0,
        selected_slot: None,
        inventory: vec![unknown.clone(); mod_api::PLAYER_STATE_INVENTORY_SLOTS],
        armor: vec![unknown.clone(); mod_api::PLAYER_STATE_ARMOR_SLOTS],
        offhand: unknown,
        effects: Vec::new(),
    }
}

fn call(denied: bool, present: bool) -> String {
    let result = format!(
        "i32.const 256 call $read i32.const 256 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(denied)
    );
    if denied {
        return result;
    }
    format!(
        "{result} i32.const 264 i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(present)
    )
}

#[test]
fn player_state_crosses_the_component_boundary_without_gameplay_and_clears_between_frames() {
    let frame = format!(
        "global.get $count i32.const 1 i32.eq if {} i32.const 272 i64.load i64.const 7 i64.ne if unreachable end else {} end",
        call(false, true),
        call(false, false),
    );
    let (_dir, mut host) = load(&call(false, false), &frame, true);
    host.frame_with_player_state(false, None, Vec::new(), Some(snapshot()), empty_controls())
        .unwrap();
    host.frame(false).unwrap();
    assert!(host.is_active());
    let (_dir, mut denied) = load(&call(true, false), &call(true, false), false);
    denied
        .frame_with_player_state(false, None, Vec::new(), Some(snapshot()), empty_controls())
        .unwrap();
    assert!(denied.is_active());
}

#[test]
fn player_state_read_budget_traps_and_reload_starts_without_previous_facts() {
    let frame = std::iter::repeat_n(call(false, true), 9)
        .collect::<Vec<_>>()
        .join(" ");
    let (dir, mut host) = load("", &frame, true);
    assert!(
        host.frame_with_player_state(false, None, Vec::new(), Some(snapshot()), empty_controls())
            .is_err()
    );
    assert!(!host.is_active());
    std::fs::write(
        dir.path().join("player-state.wat"),
        source(&call(false, false), &call(false, false)),
    )
    .unwrap();
    assert!(host.reload_if_changed().unwrap());
    host.frame(false).unwrap();
    assert!(host.is_active());
}

#[test]
fn legacy_actor_component_keeps_its_original_import_type() {
    let source = source_from(include_str!("actors_0_1.wat"), "", "");
    let directory = tempfile::tempdir().unwrap();
    let mut host = ModHost::load_snapshot_with_grants(
        &directory.path().join("legacy-actors.wat"),
        source.as_bytes(),
        ModGrants {
            players: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.frame(false).unwrap();
    assert!(host.is_active());
}

/// Requests runtime IDs 9 and 10 and traps unless 9 is a dead life 77 and 10 is unknown.
const READ_ACTOR_LIVES: &str = "i32.const 512 i64.const 9 i64.store \
    i32.const 520 i64.const 10 i64.store \
    i32.const 512 i32.const 2 i32.const 256 call $read \
    i32.const 256 i32.load8_u if unreachable end \
    i32.const 264 i32.load i32.const 2 i32.ne if unreachable end \
    i32.const 260 i32.load i32.load8_u i32.const 1 i32.ne if unreachable end \
    i32.const 260 i32.load i64.load offset=8 i64.const 77 i64.ne if unreachable end \
    i32.const 260 i32.load i32.load8_u offset=16 i32.const 1 i32.ne if unreachable end \
    i32.const 260 i32.load i32.load8_u offset=24 if unreachable end";

/// Lives cross the component boundary in request order and last only for the callback they
/// were observed for.
#[test]
fn actor_lives_cross_the_component_boundary_for_one_callback() {
    let source = source_from(include_str!("actor_lives.wat"), "", READ_ACTOR_LIVES);
    let directory = tempfile::tempdir().unwrap();
    let mut host = ModHost::load_snapshot_with_grants(
        &directory.path().join("actor-lives.wat"),
        source.as_bytes(),
        ModGrants {
            players: true,
            ..Default::default()
        },
    )
    .unwrap();
    let observations = || crate::module_observations::ModuleObservations {
        actor_lives: std::sync::Arc::new(vec![crate::module_observations::ActorLifeObservation {
            runtime_id: 9,
            player: true,
            life: crate::ActorLife {
                life: 77,
                dead: true,
            },
        }]),
        ..Default::default()
    };
    host.observe_modules(observations()).unwrap();
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert!(
        host.frame(false).is_err(),
        "the next callback without observations reads no life"
    );
}

/// Refusals cross the component boundary as errors, keeping the guest active.
#[test]
fn actor_life_grant_and_length_refusals_do_not_trap() {
    for (granted, length) in [(false, 1), (true, mod_api::MAX_ACTOR_LIFE_READS + 1)] {
        let frame = format!(
            "i32.const 512 i32.const {length} i32.const 256 call $read \
             i32.const 256 i32.load8_u i32.const 1 i32.ne if unreachable end"
        );
        let source = source_from(include_str!("actor_lives.wat"), "", &frame);
        let directory = tempfile::tempdir().unwrap();
        let mut host = ModHost::load_snapshot_with_grants(
            &directory.path().join("actor-life-refusal.wat"),
            source.as_bytes(),
            ModGrants {
                players: granted,
                ..Default::default()
            },
        )
        .unwrap();
        host.frame(false).unwrap();
        assert!(host.is_active());
    }
}

/// A guest built against the original combat record and function still instantiates.
#[test]
fn legacy_combat_component_keeps_its_original_import_type() {
    let source = source_from(include_str!("combat_0_1.wat"), "", "");
    let directory = tempfile::tempdir().unwrap();
    let mut host = ModHost::load_snapshot_with_grants(
        &directory.path().join("legacy-combat.wat"),
        source.as_bytes(),
        ModGrants {
            player_state: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.frame(false).unwrap();
    assert!(host.is_active());
}

/// Per-event lives and unknown sequences cross the component boundary in request order.
#[test]
fn combat_lives_cross_the_component_boundary_and_clear_after_the_callback() {
    let frame = "i32.const 512 i64.const 9 i64.store \
        i32.const 520 i64.const 10 i64.store \
        i32.const 512 i32.const 2 i32.const 256 call $read \
        i32.const 256 i32.load8_u if unreachable end \
        i32.const 264 i32.load i32.const 2 i32.ne if unreachable end \
        i32.const 260 i32.load i32.load8_u i32.const 1 i32.ne if unreachable end \
        i32.const 260 i32.load i64.load offset=8 i64.const 77 i64.ne if unreachable end \
        i32.const 260 i32.load i32.load8_u offset=16 if unreachable end";
    let source = source_from(include_str!("combat_lives.wat"), "", frame);
    let directory = tempfile::tempdir().unwrap();
    let mut host = ModHost::load_snapshot_with_grants(
        &directory.path().join("combat-lives.wat"),
        source.as_bytes(),
        ModGrants {
            player_state: true,
            players: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.observe_modules(crate::module_observations::ModuleObservations {
        combat: vec![crate::module_observations::RecordedCombatObservation {
            event: crate::CombatObservation {
                sequence: 9,
                session: 7,
                dimension: 0,
                kind: crate::CombatKind::Totem,
                local: false,
                player: true,
                runtime_id: 5,
                distance: None,
                position: (0., 0., 0.),
            },
            life: Some(77),
        }],
        ..Default::default()
    })
    .unwrap();
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert!(
        host.frame(false).is_err(),
        "the next callback has no retained observations"
    );
}

/// Capability and length refusals are recoverable results at the guest boundary.
#[test]
fn combat_life_grant_and_length_refusals_do_not_trap() {
    for (granted, length) in [(false, 1), (true, mod_api::MAX_COMBAT_LIFE_READS + 1)] {
        let frame = format!(
            "i32.const 512 i32.const {length} i32.const 256 call $read \
            i32.const 256 i32.load8_u i32.const 1 i32.ne if unreachable end"
        );
        let source = source_from(include_str!("combat_lives.wat"), "", &frame);
        let directory = tempfile::tempdir().unwrap();
        let mut host = ModHost::load_snapshot_with_grants(
            &directory.path().join("combat-life-refusal.wat"),
            source.as_bytes(),
            ModGrants {
                player_state: granted,
                ..Default::default()
            },
        )
        .unwrap();
        host.frame(false).unwrap();
        assert!(host.is_active());
    }
}
