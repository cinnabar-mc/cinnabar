use super::*;

/// Runs one update without granting a world-entry allowance, failing on any guest fault.
fn frame(runtime: &mut ModHosts) -> Merged {
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
            world_observation: None,
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
fn every_runnable_mod_runs_when_the_pool_only_fits_one_full_reservation() {
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
        frame(&mut runtime);
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
        let merged = frame(&mut runtime);
        assert_eq!(merged.time_override, Some(1));
        assert_eq!(merged.rig.unwrap().offset.x, 0.5);
        assert_eq!(merged.commands.len(), runtime.host_count());
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
        frame(&mut runtime);
    }
    assert!(runtime.host(1).label().is_some() && runtime.host(2).label().is_some());
    runtime.swap(0, 2);
    runtime.remove(1);
    runtime.begin_frame();
    runtime.budget.reset(mod_host::FRAME_FUEL);
    frame(&mut runtime);
    assert!(runtime.host(0).last_fuel_used() > 0);
}
