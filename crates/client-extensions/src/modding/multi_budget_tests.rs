use super::*;

/// Runs one update with optional world-entry observations, failing on any guest fault.
fn frame(runtime: &mut ModHosts, world: Option<&mod_host::WorldObservation>) -> Merged {
    let (merged, failures) = frame_result(runtime, world);
    assert!(failures.is_empty(), "guest failures: {failures:?}");
    merged
}

/// Runs one update and records guest failures so ownership after a trap can be checked.
fn frame_result(
    runtime: &mut ModHosts,
    world: Option<&mod_host::WorldObservation>,
) -> (Merged, Vec<(usize, String)>) {
    let controls = mod_host::empty_controls();
    let mut failures = Vec::new();
    let merged = run_frame(
        runtime,
        FrameInput {
            native_editing: None,
            modules: None,
            pressed: false,
            controls: &controls,
            previous_cues: &[],
            player_state: None,
            world_observation: world,
            fps: None,
            sidebar: None,
            chat: &[],
            chat_events: &[],
            gameplay_facts: Default::default(),
        },
        |_| (Some(snapshot()), Vec::new(), None),
        |index, error| failures.push((index, error)),
    );
    (merged, failures)
}

/// Writes a probe with optional bounded frame work and cue checks disabled.
fn probe(directory: &Path, label: &'static str, work: bool) -> PathBuf {
    let path = Probe {
        command: format!("/ability {label}"),
        second_frame_cues: 0,
        ..Probe::new(label, label.len() as u32, 0.5)
    }
    .write(directory);
    if work {
        let source = std::fs::read_to_string(&path).unwrap()
            // Only the core function; the component's lifted `frame` export takes no locals.
            .replacen("(func (export \"frame\")", "(func (export \"frame\") (local $work i32)", 1)
            .replace("global.get $frames i32.const 1 i32.add global.set $frames", &format!(
                "i32.const {} local.set $work (loop $spin local.get $work i32.const 1 i32.sub local.tee $work br_if $spin) global.get $frames i32.const 1 i32.add global.set $frames",
                mod_host::FRAME_FUEL / 8));
        std::fs::write(&path, source).unwrap();
    }
    path
}

#[test]
fn rotating_admission_does_not_starve_mods_when_only_one_callback_fits() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(
        ["A", "BB", "CCC", "DDDD"]
            .into_iter()
            .map(|label| (probe(directory.path(), label, true), granted()))
            .collect(),
    );
    let mut runs = vec![0; runtime.host_count()];
    for _ in 0..runtime.host_count() * 2 {
        runtime.begin_frame();
        runtime.budget.reset(mod_host::FRAME_FUEL);
        let merged = frame(&mut runtime, None);
        assert_eq!(merged.commands.len(), 1);
        for (index, runs) in runs.iter_mut().enumerate() {
            let used = runtime.host(index).last_fuel_used();
            assert!(runtime.host(index).is_active() && !runtime.host(index).is_recovering());
            if used != 0 {
                assert!(used < mod_host::FRAME_FUEL);
                *runs += 1;
            }
        }
    }
    assert_eq!(runs, vec![2; runtime.host_count()]);
}

#[test]
fn three_light_mods_all_run_at_every_priority_with_and_without_world_entry() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(
        ["A", "BB", "CCC"]
            .into_iter()
            .map(|label| (probe(directory.path(), label, false), granted()))
            .collect(),
    );
    let world = mod_host::WorldObservation {
        session: snapshot().session,
        world: "budget-fixture".into(),
        dimension: snapshot().dimension,
        position: (0., 64., 0.),
        alive: true,
        runtime_id: 1,
    };
    for observation in [None, Some(&world)] {
        for _ in 0..runtime.host_count() {
            runtime.begin_frame();
            let merged = frame(&mut runtime, observation);
            assert_eq!(
                merged.commands,
                ["/ability A", "/ability BB", "/ability CCC"]
            );
            let mut used = 0;
            for index in 0..runtime.host_count() {
                let host = runtime.host(index);
                assert!(host.is_active() && !host.is_recovering());
                assert!(host.last_fuel_used() > 0);
                used += host.last_fuel_used();
            }
            assert_eq!(
                runtime.budget.remaining(),
                mod_api::TOTAL_MOD_FRAME_FUEL - used
            );
        }
    }
}

#[test]
fn cold_rotated_execution_still_gives_first_mod_output_authority() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(
        ["A", "BB", "CCC"]
            .into_iter()
            .map(|label| (probe(directory.path(), label, false), granted()))
            .collect(),
    );
    runtime.next_frame_priority = Some(runtime.host(2).frame_owner());
    runtime.begin_frame();
    let merged = frame(&mut runtime, None);
    assert_eq!(merged.time_override, Some(1));
    assert!(runtime.host(0).has_camera_output());
    assert!(!runtime.host(1).has_camera_output());
    assert!(!runtime.host(2).has_camera_output());
    assert_eq!(runtime.host(1).time_override(), None);
    assert_eq!(runtime.host(2).time_override(), None);
    assert_eq!(
        merged.commands,
        ["/ability A", "/ability BB", "/ability CCC"]
    );
}

#[test]
fn rotated_callbacks_hand_over_exclusive_output_after_release_or_quarantine() {
    for (extra, quarantined) in [
        (
            "global.get $frames i32.const 2 i32.eq if
             i32.const 0 i32.const 0 i32.const 512 call $time
             i32.const 0 f32.const 0 f32.const 0 f32.const 0 f32.const 0 f32.const 0
             i32.const 512 call $rig end",
            false,
        ),
        (
            "global.get $frames i32.const 2 i32.eq if unreachable end",
            true,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = load_set(
            [
                Probe {
                    extra,
                    second_frame_cues: 0,
                    ..Probe::new("A", 1, 0.5)
                },
                Probe {
                    second_frame_cues: 0,
                    ..Probe::new("B", 2, -0.5)
                },
            ]
            .into_iter()
            .map(|probe| (probe.write(directory.path()), granted()))
            .collect(),
        );
        runtime.begin_frame();
        assert_eq!(frame(&mut runtime, None).time_override, Some(1));
        runtime.begin_frame();
        let (merged, failures) = frame_result(&mut runtime, None);
        assert_eq!(failures.len(), usize::from(quarantined));
        if quarantined {
            assert_eq!(failures[0].0, 0);
        }
        assert_eq!(runtime.host(0).is_active(), !quarantined);
        assert_eq!(merged.time_override, Some(2));
        assert_eq!(merged.rig.unwrap().offset.x, -0.5);
        assert!(runtime.host(1).is_active() && runtime.host(1).last_fuel_used() > 0);
    }
}

#[test]
fn simultaneous_panel_requests_keep_load_order_precedence_at_every_priority() {
    let directory = tempfile::tempdir().unwrap();
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let source = r#"(component
      (import "$PANEL" (instance $panel
        (export "open" (func (result (result (error string)))))))
      (alias export $panel "open" (func $open))
      (core module $memory-module
        (memory (export "memory") 1)
        (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
      (core instance $mem (instantiate $memory-module))
      (alias core export $mem "memory" (core memory $memory))
      (alias core export $mem "realloc" (core func $realloc))
      (core func $lower-open (canon lower (func $open) (memory $memory) (realloc $realloc)))
      (core module $code
        (import "host" "open" (func $open (param i32)))
        (import "host" "memory" (memory 1))
        (func (export "init"))
        (func (export "frame") i32.const 0 call $open
          i32.const 0 i32.load8_u if unreachable end))
      (core instance $host (export "open" (func $lower-open)) (export "memory" (memory $memory)))
      (core instance $run (instantiate $code (with "host" (instance $host))))
      (func (export "init") (canon lift (core func $run "init")))
      (func (export "frame") (canon lift (core func $run "frame"))))"#
        .replace("$PANEL", &format!("{name}/panel@{version}"));
    let mut runtime = load_set(
        (0..2)
            .map(|index| {
                let path = directory.path().join(format!("panel-{index}.wat"));
                std::fs::write(&path, &source).unwrap();
                (
                    path,
                    ModGrants {
                        controls: true,
                        ..Default::default()
                    },
                )
            })
            .collect(),
    );
    let bundle = serde_json::json!({
        "panel": {"controls":[], "surface":{"screen":"test.root",
            "document":r#"{"namespace":"test","root":{"type":"panel"}}"#}},
        "surfaces":{}
    });
    for index in 0..runtime.host_count() {
        runtime
            .host_mut(index)
            .set_panel_bundle(&bundle.to_string())
            .unwrap();
    }
    let mut previous = runtime.latest_panel_open_order();
    for _ in 0..runtime.host_count() * 2 {
        runtime.begin_frame();
        frame(&mut runtime, None);
        assert_eq!(runtime.panel_owner(), 1);
        assert!(!runtime.host(0).panel_open() && runtime.host(1).panel_open());
        let newest = (0..runtime.host_count())
            .map(|index| runtime.host(index).panel_open_order())
            .max()
            .unwrap();
        assert_eq!(runtime.latest_panel_open_order(), newest);
        assert!(newest > previous);
        previous = newest;
    }
}

#[test]
fn empty_pool_does_not_use_up_the_next_mods_turn() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(
        ["A", "BB", "CCC"]
            .into_iter()
            .map(|label| (probe(directory.path(), label, true), granted()))
            .collect(),
    );
    for (pool, expected) in [
        (mod_host::FRAME_FUEL, vec!["/ability A"]),
        (0, vec![]),
        (mod_host::FRAME_FUEL, vec!["/ability BB"]),
    ] {
        runtime.begin_frame();
        runtime.budget.reset(pool);
        assert_eq!(frame(&mut runtime, None).commands, expected);
    }
}

#[test]
fn rotating_admission_preserves_load_order_authority_and_output_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(
        ["A", "BB", "CCC"]
            .into_iter()
            .map(|label| (probe(directory.path(), label, false), granted()))
            .collect(),
    );
    for _ in 0..runtime.host_count() * 2 {
        let refused = runtime.host(1).refused_outputs();
        runtime.begin_frame();
        let merged = frame(&mut runtime, None);
        assert_eq!(merged.time_override, Some(1));
        assert_eq!(merged.rig.unwrap().offset.x, 0.5);
        assert_eq!(
            merged.commands,
            ["/ability A", "/ability BB", "/ability CCC"]
        );
        assert_eq!(runtime.merged_label(), Some("A | BB | CCC"));
        assert_eq!(runtime.host(0).refused_outputs(), 0);
        assert_eq!(runtime.host(1).refused_outputs(), refused + 2);
    }
}

#[test]
fn priority_skips_inactive_hosts_and_survives_reordering_and_removal() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(
        ["A", "BB", "CCC"]
            .into_iter()
            .map(|label| (probe(directory.path(), label, true), granted()))
            .collect(),
    );
    runtime.host_mut(0).revoke();
    for _ in 0..2 {
        runtime.begin_frame();
        runtime.budget.reset(mod_host::FRAME_FUEL);
        frame(&mut runtime, None);
    }
    assert!(runtime.host(1).label().is_some() && runtime.host(2).label().is_some());
    runtime.swap(0, 2);
    runtime.remove(1);
    runtime.begin_frame();
    runtime.budget.reset(mod_host::FRAME_FUEL);
    frame(&mut runtime, None);
    assert!(runtime.host(0).last_fuel_used() > 0);
}

/// Gives only the first mod a deterministic overshoot, then stops the scheduling clock.
fn indebted_set(directory: &Path) -> ModHosts {
    let mut runtime = load_set(
        ["A", "BB", "CCC"]
            .into_iter()
            .map(|label| (probe(directory, label, false), granted()))
            .collect(),
    );
    let samples = std::sync::atomic::AtomicU64::new(0);
    runtime.budget =
        mod_host::CallbackBudget::with_clock(mod_api::TOTAL_MOD_FRAME_FUEL, move || {
            if samples.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                std::time::Duration::ZERO
            } else {
                std::time::Duration::from_millis(20)
            }
        });
    runtime.begin_frame();
    assert_eq!(frame(&mut runtime, None).commands, ["/ability A"]);
    runtime
}

#[test]
fn reordering_keeps_time_debt_with_the_expensive_instance() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = indebted_set(directory.path());
    runtime.swap(0, 1);
    runtime.begin_frame();
    assert_eq!(
        frame(&mut runtime, None).commands,
        ["/ability BB", "/ability CCC"]
    );
    assert_eq!(runtime.host(1).last_fuel_used(), 0);
}

#[test]
fn removing_a_non_tail_mod_does_not_transfer_its_time_debt() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = indebted_set(directory.path());
    let removed = runtime.remove(0);
    runtime.begin_frame();
    assert_eq!(
        frame(&mut runtime, None).commands,
        ["/ability BB", "/ability CCC"]
    );
    assert!(runtime.host(0).last_fuel_used() > 0);
    drop(removed);
}

#[test]
fn replacing_a_host_in_place_starts_a_fresh_time_account() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = indebted_set(directory.path());
    let replacement = probe(directory.path(), "Replacement", false);
    *runtime.host_mut(0) = ModHost::load_with_grants(&replacement, granted()).unwrap();
    runtime.begin_frame();
    assert_eq!(
        frame(&mut runtime, None).commands,
        ["/ability Replacement", "/ability BB", "/ability CCC"]
    );
}

#[test]
fn hot_reload_drops_old_debt_only_after_a_successful_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = indebted_set(directory.path());
    let original = runtime.host(0).frame_owner();
    let path = directory.path().join("A.wat");
    std::fs::write(&path, "(component)").unwrap();
    assert!(runtime.host_mut(0).reload_if_changed().is_err());
    assert_eq!(runtime.host(0).frame_owner(), original);
    runtime.begin_frame();
    assert_eq!(
        frame(&mut runtime, None).commands,
        ["/ability BB", "/ability CCC"]
    );
    assert_eq!(runtime.host(0).last_fuel_used(), 0);

    let replacement = probe(directory.path(), "Reloaded", false);
    std::fs::copy(replacement, path).unwrap();
    assert!(runtime.host_mut(0).reload_if_changed().unwrap());
    assert_ne!(runtime.host(0).frame_owner(), original);
    runtime.begin_frame();
    assert_eq!(
        frame(&mut runtime, None).commands,
        ["/ability Reloaded", "/ability BB", "/ability CCC"]
    );
}

#[test]
fn reordering_preserves_the_next_instances_admission_turn() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(
        ["A", "BB", "CCC"]
            .into_iter()
            .map(|label| (probe(directory.path(), label, true), granted()))
            .collect(),
    );
    runtime.begin_frame();
    runtime.budget.reset(mod_host::FRAME_FUEL);
    assert_eq!(frame(&mut runtime, None).commands, ["/ability A"]);
    runtime.swap(1, 2);
    runtime.begin_frame();
    runtime.budget.reset(mod_host::FRAME_FUEL);
    assert_eq!(frame(&mut runtime, None).commands, ["/ability BB"]);
    runtime.remove(0);
    runtime.begin_frame();
    runtime.budget.reset(mod_host::FRAME_FUEL);
    assert_eq!(frame(&mut runtime, None).commands, ["/ability CCC"]);
}
