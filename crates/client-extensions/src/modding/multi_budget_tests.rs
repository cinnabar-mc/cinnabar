use super::*;

/// Runs one update with optional world-entry observations, failing on any guest fault.
fn frame(runtime: &mut ModHosts, world: Option<&mod_host::WorldObservation>) -> Merged {
    let controls = mod_host::empty_controls();
    run_frame(
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
        |index, error| panic!("mod {index} failed: {error}"),
    )
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
            .replace("(func (export \"frame\")", "(func (export \"frame\") (local $work i32)")
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
    runtime.next_frame_priority = 2;
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
