use {super::*, mod_api::MAX_LOADED_MODS};

const EMPTY_COMPONENT: &str = r#"(component
    (core module $m (func (export "init")) (func (export "frame")))
    (core instance $i (instantiate $m))
    (func (export "init") (canon lift (core func $i "init")))
    (func (export "frame") (canon lift (core func $i "frame"))))"#;

/// Writes a minimal component fixture to the selected directory.
fn component(directory: &Path, name: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, EMPTY_COMPONENT).unwrap();
    path
}

#[test]
fn keys_reserved_by_an_earlier_mod_are_withheld_and_panel_events_go_to_the_owner() {
    let mut frame = mod_host::empty_controls();
    frame.keys_pressed = vec!["Digit1".into(), "KeyQ".into()];
    frame.keys_held = vec!["Digit1".into(), "ShiftLeft".into()];
    frame.keys_admitted = vec!["Digit1".into(), "ShiftLeft".into()];
    frame.events = vec![mod_host::ControlEvent {
        id: "enabled".into(),
        value: 1.0,
    }];
    frame.pointer = Some((40.0, 12.5));
    frame.wheel = -2.0;
    let later = claim_controls(&frame, &["Digit1".into()], false, &[]);
    assert_eq!(later.keys_pressed, ["KeyQ"]);
    assert_eq!(later.keys_held, ["ShiftLeft"]);
    // Current-callback reservations do not rewrite the already finalized native gate.
    assert_eq!(later.keys_admitted, frame.keys_admitted);
    assert!(later.events.is_empty());
    assert_eq!((later.pointer, later.wheel), (None, 0.0));
    let owner = claim_controls(&frame, &[], true, &[]);
    assert_eq!(owner.keys_pressed, frame.keys_pressed);
    assert_eq!(owner.events.len(), 1);
    assert_eq!((owner.pointer, owner.wheel), (Some((40.0, 12.5)), -2.0));
}

/// Chat clicks reach the chat-action owner whether or not it owns the panel, within the
/// shared event bound.
#[test]
fn chat_clicks_go_to_the_chat_action_owner() {
    let mut frame = mod_host::empty_controls();
    frame.events = vec![mod_host::ControlEvent {
        id: "toggle".into(),
        value: 1.0,
    }];
    let click = mod_host::ControlEvent {
        id: "help".into(),
        value: 1.0,
    };
    let chat_only = claim_controls(&frame, &[], false, std::slice::from_ref(&click));
    assert_eq!(chat_only.events, std::slice::from_ref(&click));
    let both = claim_controls(&frame, &[], true, std::slice::from_ref(&click));
    assert_eq!(both.events, [frame.events[0].clone(), click.clone()]);
    let flood = vec![click; ui::mod_panel::MAX_PANEL_EVENTS + 4];
    assert_eq!(
        claim_controls(&frame, &[], true, &flood).events.len(),
        ui::mod_panel::MAX_PANEL_EVENTS
    );
}

/// Writes a presentation probe that sets a body hurt colour and one chat action.
fn presentation_probe(directory: &Path, action: &str, red: f32) -> PathBuf {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let source = include_str!("presentation_probe.wat")
        .replace("$PRESENTATION", &format!("{name}/presentation@{version}"))
        .replace("$ACTION_LENGTH", &action.len().to_string())
        .replace("$ACTION", action)
        .replace("$RED", &red.to_string());
    let path = directory.join(format!("{action}.wat"));
    std::fs::write(&path, source).unwrap();
    path
}

/// The earliest active mod owns merged presentation policies and chat clicks; a failed or
/// reordered owner hands them to the next mod, and an unchanged set reuses the merge.
#[test]
fn merged_presentation_follows_load_order_and_caches_unchanged_output() {
    let directory = tempfile::tempdir().unwrap();
    let grants = ModGrants {
        hud: true,
        ..Default::default()
    };
    let mut runtime = load_set(vec![
        (
            presentation_probe(directory.path(), "first", 1.0),
            grants.clone(),
        ),
        (presentation_probe(directory.path(), "second", 0.5), grants),
    ]);
    let controls = mod_host::empty_controls();
    let frame = |runtime: &mut ModHosts| {
        runtime.begin_frame();
        run_frame(
            runtime,
            FrameInput {
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
            |_| (None, Vec::new(), None),
            |index, error| panic!("mod {index} failed: {error}"),
        );
    };
    assert!(runtime.merged_presentation().chat_actions.is_empty());
    frame(&mut runtime);
    let merged = runtime.merged_presentation();
    assert_eq!(merged.hurt_overlay.body, Some([1., 0., 0., 1.]));
    assert_eq!(merged.chat_actions[0].action, "first");
    assert_eq!(runtime.chat_action_owner(), Some(0));
    frame(&mut runtime);
    assert!(std::sync::Arc::ptr_eq(
        &merged,
        &runtime.merged_presentation()
    ));
    runtime.swap(0, 1);
    let swapped = runtime.merged_presentation();
    assert_eq!(swapped.hurt_overlay.body, Some([0.5, 0., 0., 1.]));
    assert_eq!(swapped.chat_actions[0].action, "second");
    assert_eq!(runtime.chat_action_owner(), Some(0));
    runtime.remove(0);
    assert_eq!(
        runtime.merged_presentation().chat_actions[0].action,
        "first"
    );
}

#[test]
fn labels_join_in_load_order_within_the_plain_text_limit() {
    assert_eq!(join_labels(&[]), None);
    let labels = ["Lock-on".to_owned(), "FX".to_owned()];
    assert_eq!(join_labels(&labels).as_deref(), Some("Lock-on | FX"));
    let label = join_labels(&["é".repeat(mod_host::MAX_LABEL_BYTES)]).unwrap();
    assert!(label.len() <= mod_host::MAX_LABEL_BYTES && label.chars().all(|c| c == 'é'));
}

#[test]
fn set_files_keep_order_and_per_mod_grants() {
    let directory = tempfile::tempdir().unwrap();
    let set = directory.path().join("mods.json");
    let write = |json: &str| std::fs::write(&set, json).unwrap();
    let camera = directory.path().join("camera.wasm");
    let effects = directory.path().join("effects.wasm");
    write(
        &serde_json::json!({"version":1,"mods":[
            {"component":camera,"grants":{"camera":true,"commands":["ability"]}},
            {"component":effects,"grants":{"environment":true}}
        ]})
        .to_string(),
    );
    let mods = read_set(&set).unwrap();
    assert_eq!(mods[0].0, ModSource::Component(camera));
    assert!(mods[0].1.camera && !mods[0].1.environment);
    assert_eq!(mods[0].1.commands, ["ability"]);
    assert!(mods[1].1.environment && !mods[1].1.camera);

    for invalid in [
        r#"{"version":2,"mods":[{"component":"/a.wasm"}]}"#,
        r#"{"version":1,"mods":[]}"#,
        r#"{"version":1,"mods":[{"component":"relative.wasm"}]}"#,
        r#"{"version":1,"mods":[{"component":"/a.wasm","grants":{"root":true}}]}"#,
    ] {
        write(invalid);
        assert!(read_set(&set).is_err(), "{invalid}");
    }
    let many = format!(
        r#"{{"version":1,"mods":[{}]}}"#,
        [r#"{"component":"/a.wasm"}"#; MAX_LOADED_MODS + 1].join(",")
    );
    write(&many);
    assert!(read_set(&set).is_err());
}

#[test]
fn set_entries_name_one_component_or_one_package() {
    let directory = tempfile::tempdir().unwrap();
    let set = directory.path().join("mods.json");
    let bei = directory.path().join("bei");
    let lock = directory.path().join("lock.wasm");
    std::fs::write(
        &set,
        serde_json::json!({"version":1,"mods":[
            {"package":bei,"grants":{"screen":true,"items":true}},
            {"component":lock}
        ]})
        .to_string(),
    )
    .unwrap();
    let mods = read_set(&set).unwrap();
    assert_eq!(mods[0].0, ModSource::Package(bei.clone()));
    assert!(mods[0].1.screen && mods[0].1.items && !mods[0].1.keys);
    assert_eq!(mods[1].0, ModSource::Component(lock.clone()));
    for invalid in [
        serde_json::json!({"version":1,"mods":[{"package":bei,"component":lock}]}),
        serde_json::json!({"version":1,"mods":[{"grants":{"screen":true}}]}),
    ] {
        std::fs::write(&set, invalid.to_string()).unwrap();
        assert!(read_set(&set).is_err(), "{invalid}");
    }
}

#[test]
fn the_earliest_drawing_package_owns_the_screens() {
    // (may draw, draws) in load order.
    assert_eq!(
        screen_owner([(false, false), (true, false)].into_iter()),
        Some(1)
    );
    assert_eq!(
        screen_owner([(true, false), (true, true), (true, true)].into_iter()),
        Some(1)
    );
    // A mod without the screen grant never owns, whatever it committed.
    assert_eq!(
        screen_owner([(false, true), (true, false)].into_iter()),
        Some(1)
    );
    assert_eq!(screen_owner([(false, false)].into_iter()), None);
}

#[test]
fn a_set_loads_every_valid_component_in_order_and_skips_broken_ones() {
    let directory = tempfile::tempdir().unwrap();
    let broken = directory.path().join("broken.wat");
    std::fs::write(&broken, "(component").unwrap();
    let runtime = load_set(vec![
        (broken, ModGrants::default()),
        (
            component(directory.path(), "camera.wat"),
            ModGrants {
                camera: true,
                ..Default::default()
            },
        ),
        (
            component(directory.path(), "effects.wat"),
            ModGrants {
                environment: true,
                ..Default::default()
            },
        ),
    ]);
    assert_eq!(runtime.host_count(), 2);
    assert!(runtime.host(0).grants().camera);
    assert!(runtime.host(1).grants().environment);
    assert_eq!(runtime.panel_owner(), 0);
    assert!(runtime.reserved_keys().is_empty());
}

/// Loads three distinct panel publishers so selection can be followed through order changes.
fn panel_hosts(directory: &Path) -> ModHosts {
    let mut runtime = load_set(
        ["first.wat", "second.wat", "third.wat"]
            .into_iter()
            .map(|name| {
                (
                    component(directory, name),
                    ModGrants {
                        controls: true,
                        ..Default::default()
                    },
                )
            })
            .collect(),
    );
    for (index, key) in ["KeyA", "KeyB", "KeyC"].into_iter().enumerate() {
        let bundle = serde_json::json!({
            "panel": {"toggle_key":key, "controls":[], "surface":{"screen":format!("{key}.root"),
                "document":serde_json::json!({"namespace":key,"root":{"type":"panel"}}).to_string()}},
            "surfaces":{}
        });
        runtime
            .host_mut(index)
            .set_panel_bundle(&bundle.to_string())
            .unwrap();
    }
    runtime
}

#[test]
fn removing_the_first_host_keeps_surviving_order_and_selected_panel() {
    let directory = tempfile::tempdir().unwrap();
    for (key, selected) in [("KeyA", 0), ("KeyB", 0), ("KeyC", 1)] {
        let mut runtime = panel_hosts(directory.path());
        runtime.select_panel(&[key.into()]);
        let removed = runtime.remove(0);
        assert_eq!(removed.panel().unwrap().toggle_key, "KeyA");
        assert_eq!(runtime.host_count(), 2);
        assert_eq!(runtime.host(0).panel().unwrap().toggle_key, "KeyB");
        assert_eq!(runtime.host(1).panel().unwrap().toggle_key, "KeyC");
        assert_eq!(runtime.panel_owner(), selected);
    }
}

#[test]
fn reordering_hosts_keeps_panel_selection_on_its_instance() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = panel_hosts(directory.path());
    assert!(runtime.select_panel(&["KeyC".into()]));
    runtime.host_mut(2).set_panel_open(true);
    runtime.swap(0, 2);
    assert_eq!(runtime.panel_owner(), 0);
    assert!(runtime.host(0).panel_open());
    runtime.swap(1, 0);
    assert_eq!(runtime.panel_owner(), 1);
    assert!(runtime.host(1).panel_open());
    runtime.remove(2);
    assert_eq!(runtime.panel_owner(), 1);
    runtime.remove(0);
    assert_eq!(runtime.panel_owner(), 0);
    assert_eq!(runtime.host(0).panel().unwrap().toggle_key, "KeyC");
    assert!(runtime.host(0).panel_open());
}

/// A mod that opens its panel after a tap becomes the selected panel, unless another panel is
/// capturing keys.
#[test]
fn a_tapped_mod_that_opens_its_panel_takes_the_selection() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = panel_hosts(directory.path());
    runtime.host_mut(0).set_panel_open(true);
    assert!(!runtime.select_opened_panel(1));
    runtime.host_mut(1).set_panel_open(true);
    assert!(runtime.select_opened_panel(1));
    assert_eq!(runtime.panel_owner(), 1);
    assert!(!runtime.host(0).panel_open());
    assert!(!runtime.select_opened_panel(1));
    assert!(!runtime.select_opened_panel(7));
}

#[test]
#[should_panic(expected = "a mod host collection must stay nonempty")]
fn removing_the_last_host_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(vec![(
        component(directory.path(), "only.wat"),
        ModGrants::default(),
    )]);
    runtime.remove(0);
}

/// A guest that labels, sets time, a rig and a command, emits a cue, and checks polled cues.
struct Probe {
    label: &'static str,
    ticks: u32,
    rig_x: f32,
    command: &'static str,
    second_frame_cues: usize,
    extra: &'static str,
}

impl Probe {
    /// Creates a component probe with the selected presentation output.
    fn new(label: &'static str, ticks: u32, rig_x: f32) -> Self {
        Self {
            label,
            ticks,
            rig_x,
            command: "/ability flash",
            second_frame_cues: 2,
            extra: "",
        }
    }

    /// Writes this probe using the guest SDK package version.
    fn write(&self, directory: &Path) -> PathBuf {
        let package = include_str!("../../../mod-api/wit/extension.wit")
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("package ")
            .trim_end_matches(';');
        let (name, version) = package.split_once('@').unwrap();
        let source = include_str!("multi_probe.wat")
            .replace("$HUD", &format!("{name}/hud@{version}"))
            .replace("$ENVIRONMENT", &format!("{name}/environment@{version}"))
            .replace("$GAMEPLAY", &format!("{name}/gameplay@{version}"))
            .replace("$EVENTS", &format!("{name}/events@{version}"))
            .replace("$LABEL_LENGTH", &self.label.len().to_string())
            .replace("$LABEL", self.label)
            .replace("$COMMAND_LENGTH", &self.command.len().to_string())
            .replace("$COMMAND", self.command)
            .replace("$TICKS", &self.ticks.to_string())
            .replace("$RIG_X", &self.rig_x.to_string())
            .replace("$SECOND_FRAME_CUES", &self.second_frame_cues.to_string())
            .replace("$EXTRA", self.extra);
        let path = directory.join(format!("{}.wat", self.label));
        std::fs::write(&path, source).unwrap();
        path
    }
}

/// Grants the capabilities exercised by the load-order probes.
fn granted() -> ModGrants {
    ModGrants {
        environment: true,
        camera: true,
        commands: vec!["ability".into()],
        ..Default::default()
    }
}

/// Supplies one connected gameplay frame for the probes.
fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: mod_host::GameplayVector3 {
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

/// Loads `probes` as one set and runs `frames` frames, returning the last merge and failures.
fn run(probes: &[Probe], frames: usize) -> (ModHosts, Merged, Vec<usize>) {
    let directory = tempfile::tempdir().unwrap();
    let set = probes
        .iter()
        .map(|probe| (probe.write(directory.path()), granted()))
        .collect();
    let mut runtime = load_set(set);
    let controls = mod_host::empty_controls();
    let mut previous = Vec::new();
    let mut failures = Vec::new();
    let mut merged = Merged::default();
    let mut captures = 0;
    for _ in 0..frames {
        runtime.begin_frame();
        merged = run_frame(
            &mut runtime,
            FrameInput {
                modules: None,
                pressed: false,
                controls: &controls,
                previous_cues: &previous,
                player_state: None,
                world_observation: None,
                fps: None,
                sidebar: None,
                chat: &[],
                chat_events: &[],
                gameplay_facts: Default::default(),
            },
            |_| {
                captures += 1;
                (Some(snapshot()), Vec::new(), None)
            },
            |index, _| failures.push(index),
        );
        previous = merged.cues.clone();
    }
    assert_eq!(captures, frames);
    (runtime, merged, failures)
}

#[test]
fn load_order_decides_single_valued_outputs_and_keeps_every_command() {
    let (mut runtime, merged, failures) = run(
        &[Probe::new("A", 1000, 0.5), Probe::new("B", 6000, -0.5)],
        2,
    );
    assert!(failures.is_empty());
    assert_eq!(merged.time_override, Some(1000));
    assert_eq!(runtime.host(0).refused_outputs(), 0);
    assert!(runtime.host(1).refused_outputs() >= 2);
    assert_eq!(merged.rig.unwrap().offset.x, 0.5);
    assert_eq!(runtime.merged_label(), Some("A | B"));
    assert_eq!(merged.commands, ["/ability flash", "/ability flash"]);

    let (mut runtime, merged, _) = run(
        &[Probe::new("B", 6000, -0.5), Probe::new("A", 1000, 0.5)],
        2,
    );
    assert_eq!(merged.time_override, Some(6000));
    assert_eq!(merged.rig.unwrap().offset.x, -0.5);
    assert_eq!(runtime.merged_label(), Some("B | A"));
}

#[test]
fn every_mod_polls_all_mods_cues_from_the_previous_frame() {
    // Each probe traps on frame two unless it polls both mods' frame-one cues.
    let (_, merged, failures) = run(&[Probe::new("A", 1000, 0.5), Probe::new("B", 6000, 0.5)], 3);
    assert!(failures.is_empty(), "a probe saw the wrong cues");
    assert_eq!(merged.cues.len(), 2);
}

#[test]
fn a_trapping_mod_is_quarantined_without_disturbing_the_others() {
    let crashing = Probe {
        extra: "unreachable",
        ..Probe::new("A", 1000, 0.5)
    };
    let survivor = Probe {
        second_frame_cues: 1,
        ..Probe::new("B", 6000, -0.5)
    };
    let (mut runtime, merged, failures) = run(&[crashing, survivor], 3);
    assert_eq!(
        failures,
        [0],
        "the trap is reported once, then the guest stays quarantined"
    );
    assert!(!runtime.host(0).is_active() && runtime.host(1).is_active());
    assert_eq!(merged.time_override, Some(6000));
    assert_eq!(merged.rig.unwrap().offset.x, -0.5);
    assert_eq!(runtime.merged_label(), Some("B"));
    assert_eq!(merged.commands, ["/ability flash"]);
}

#[test]
fn grants_stay_per_mod() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = load_set(vec![
        (
            Probe::new("A", 1000, 0.5).write(directory.path()),
            ModGrants::default(),
        ),
        (
            Probe::new("B", 6000, -0.5).write(directory.path()),
            granted(),
        ),
    ]);
    let controls = mod_host::empty_controls();
    runtime.begin_frame();
    let merged = run_frame(
        &mut runtime,
        FrameInput {
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
        |_, error| panic!("{error}"),
    );
    // The ungranted first mod cannot claim time, rig or commands ahead of the granted one.
    assert_eq!(merged.time_override, Some(6000));
    assert_eq!(merged.rig.unwrap().offset.x, -0.5);
    assert_eq!(merged.commands, ["/ability flash"]);
}

/// A set is bounded independently of its number of entries.
#[test]
fn oversized_set_is_rejected_without_reading_it_whole() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mods.json");
    std::fs::write(&path, vec![b' '; MAX_SET_BYTES + 1]).unwrap();
    let error = read_set(&path).unwrap_err();
    assert!(error.contains("byte limit"), "{error}");
}

/// Nonregular set sources fail admission without waiting for another process.
#[cfg(unix)]
#[test]
fn a_set_source_must_be_a_regular_file() {
    let directory = tempfile::tempdir().unwrap();
    let fifo = directory.path().join("mods.json");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!("Missing fixture: mkfifo is unavailable");
        return;
    }
    let error = read_set(&fifo).unwrap_err();
    assert!(error.contains("regular file"), "{error}");
}

#[test]
fn the_merged_label_is_rebuilt_only_when_a_mod_label_changes() {
    let (mut runtime, _, _) = run(
        &[Probe::new("A", 1000, 0.5), Probe::new("B", 6000, -0.5)],
        1,
    );
    assert_eq!(runtime.merged_label(), Some("A | B"));
    let built = runtime.label_rebuilds;
    for _ in 0..3 {
        assert_eq!(runtime.merged_label(), Some("A | B"));
    }
    assert_eq!(
        runtime.label_rebuilds, built,
        "unchanged labels must not be rejoined"
    );
}

#[test]
fn inactive_hosts_do_not_capture_world_state() {
    let crashing = Probe {
        extra: "unreachable",
        ..Probe::new("A", 1000, 0.5)
    };
    let (mut runtime, _, failures) = run(&[crashing], 1);
    assert_eq!(failures, [0]);
    let controls = mod_host::empty_controls();
    run_frame(
        &mut runtime,
        FrameInput {
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
        |_| panic!("inactive hosts must not capture the world"),
        |_, _| panic!("quarantined host ran again"),
    );
}

/// Loads the same component entries as the app without its presentation adapters.
fn load_set(mods: Vec<(PathBuf, ModGrants)>) -> ModHosts {
    let mut hosts = load_entries(
        mods.into_iter()
            .map(|(path, grants)| (ModSource::Component(path), grants))
            .collect(),
    )
    .into_iter();
    ModHosts::new(hosts.next().unwrap(), hosts.collect())
}

#[test]
fn aggregate_frame_fuel_stops_later_callbacks_without_quarantining_skipped_mods() {
    let directory = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..3)
        .map(|index| {
            let path = directory.path().join(format!("spin-{index}.wat"));
            let source = EMPTY_COMPONENT.replace(
                "(func (export \"frame\"))",
                "(func (export \"frame\") (loop $spin br $spin))",
            );
            std::fs::write(&path, source).unwrap();
            (path, ModGrants::default())
        })
        .collect();
    let mut runtime = load_set(paths);
    runtime.begin_frame();
    let controls = mod_host::empty_controls();
    let mut failures = Vec::new();
    run_frame(
        &mut runtime,
        FrameInput {
            pressed: false,
            controls: &controls,
            previous_cues: &[],
            player_state: None,
            world_observation: None,
            modules: None,
            fps: None,
            sidebar: None,
            chat: &[],
            chat_events: &[],
            gameplay_facts: Default::default(),
        },
        |_| (None, Vec::new(), None),
        |index, _| failures.push(index),
    );
    assert_eq!(failures, [0, 1]);
    assert!(runtime.host(0).is_active() && runtime.host(1).is_active());
    assert!(runtime.host(2).is_active());
    assert_eq!(runtime.budget.remaining(), 0);
    assert_eq!(runtime.host(2).last_fuel_used(), 0);
    runtime.begin_frame();
    assert_eq!(runtime.budget.remaining(), mod_api::TOTAL_MOD_FRAME_FUEL);
}

#[test]
fn a_higher_mod_releasing_its_outputs_allows_the_next_mod_to_publish() {
    let idle = Probe {
        extra: "i32.const 0 i32.const 0 i32.const 512 call $time i32.const 0 f32.const 0 f32.const 0 f32.const 0 f32.const 0 f32.const 0 i32.const 512 call $rig",
        ..Probe::new("A", 1000, 0.5)
    };
    let (runtime, merged, failures) = run(&[idle, Probe::new("B", 6000, -0.5)], 1);
    assert!(failures.is_empty());
    assert_eq!(merged.time_override, Some(6000));
    assert_eq!(merged.rig.unwrap().offset.x, -0.5);
    assert_eq!(runtime.host(1).refused_outputs(), 0);
}

/// Ownership follows granted opt-in instances across reorder, trap, and removal.
#[test]
fn action_filter_owner_tracks_instance_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let ordinary = ModHost::load(&component(dir.path(), "ordinary.wat")).unwrap();
    let source = include_str!("../../../mod-host/src/tests/action_filter.wat")
        .replace("$BODY", "")
        .replace("$COUNT", "i32.const 0");
    let path = dir.path().join("filter.wat");
    std::fs::write(&path, &source).unwrap();
    let grant = ModGrants {
        controls: true,
        ..Default::default()
    };
    let first = ModHost::load_with_grants(&path, grant.clone()).unwrap();
    let second = ModHost::load_with_grants(&path, grant).unwrap();
    let mut hosts = ModHosts::new(ordinary, vec![first, second]);
    assert_eq!(hosts.action_filter_owner(), Some(1));
    let identity = hosts.host(1).action_filter_identity();
    hosts.swap(0, 1);
    assert_eq!(hosts.action_filter_owner(), Some(0));
    assert_eq!(hosts.host(0).action_filter_identity(), identity);
    hosts.remove(0);
    assert_eq!(hosts.action_filter_owner(), Some(1));
    let failed = hosts.host_mut(1).filter_actions(mod_host::ActionFrame {
        epoch: 1,
        time_us: 0,
        physical: mod_host::ActionHeld {
            attack: false,
            item_use: false,
        },
        logical: mod_host::ActionHeld {
            attack: false,
            item_use: false,
        },
        transitions: vec![
            mod_host::ActionTransition {
                action: mod_host::FilteredAction::Attack,
                kind: mod_host::TransitionKind::Press,
                held_after: true,
            };
            9
        ],
    });
    assert!(failed.is_err());
    assert!(hosts.action_filter_owner().is_none());
}

/// A HUD-granted component hiding `elements`, as byte enum discriminants, in its init.
fn hiding_component(directory: &Path, name: &str, elements: &[u8]) -> PathBuf {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (namespace, version) = package.split_once('@').unwrap();
    let data: String = elements
        .iter()
        .map(|byte| format!("\\{byte:02x}"))
        .collect();
    let source = format!(
        r#"(component
  (type $e (enum "crosshair" "status-effects" "sidebar" "hunger"))
  (import "{namespace}/hud@{version}" (instance $hud
    (export "vanilla-element" (type $ve (eq $e)))
    (export "set-hidden-elements" (func (param "elements" (list $ve)) (result (result (error string)))))))
  (alias export $hud "set-hidden-elements" (func $hide))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 1024))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-hide (canon lower (func $hide) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "hide" (func $hide (param i32 i32 i32)))
    (import "host" "memory" (memory 1))
    (data (i32.const 0) "{data}")
    (func (export "init") i32.const 0 i32.const {count} i32.const 512 call $hide)
    (func (export "frame")))
  (core instance $host (export "hide" (func $lower-hide)) (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))"#,
        count = elements.len()
    );
    let path = directory.join(name);
    std::fs::write(&path, source).unwrap();
    path
}

/// Several mods' hidden vanilla elements combine, and every host reads the same HUD root.
#[test]
fn hidden_hud_elements_combine_across_mods_and_hosts_share_hud_observations() {
    use ui::mod_hud::{ElementState, HudViewport, VanillaElement};
    let directory = tempfile::tempdir().unwrap();
    let grants = ModGrants {
        hud: true,
        ..Default::default()
    };
    let first = ModSource::Component(hiding_component(directory.path(), "a.wat", &[2]))
        .load(grants.clone())
        .unwrap();
    let second = ModSource::Component(hiding_component(directory.path(), "b.wat", &[0, 3]))
        .load(grants)
        .unwrap();
    let denied = ModSource::Component(hiding_component(directory.path(), "c.wat", &[1]))
        .load(ModGrants::default())
        .unwrap();
    let mut hosts = ModHosts::new(first, vec![second, denied]);
    assert_eq!(
        hosts.hidden_hud_elements().iter().collect::<Vec<_>>(),
        [
            VanillaElement::Crosshair,
            VanillaElement::Sidebar,
            VanillaElement::Hunger
        ]
    );
    hosts.host_mut(1).revoke();
    assert_eq!(
        hosts.hidden_hud_elements().iter().collect::<Vec<_>>(),
        [VanillaElement::Sidebar],
        "a revoked mod's hiding is withdrawn"
    );
    let viewport = HudViewport {
        width: 427.,
        height: 240.,
        scale: 3.,
    };
    hosts.observe_hud(Some(viewport), [ElementState::default(); 4], None);
    assert!(hosts.host(0).hud_surface().is_none());
}

#[test]
fn unchanged_hud_metrics_are_installed_on_replacement_hosts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("measure.wat");
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let source = include_str!("../../../mod-host/src/tests/hud_surface.wat")
        .replace("$HUD", &format!("{name}/hud@{version}"))
        .replace("$SURFACE_LENGTH", "0")
        .replace("$PATCH_LENGTH", "0")
        .replace("$TEXT_LENGTH", "0")
        .replace("$SURFACE", "")
        .replace("$PATCH", "")
        .replace("$TEXT", "")
        .replace("$WIDTH", "427")
        .replace("$SCALE", "3")
        .replace("$INIT", "")
        .replace("$FRAME", "call $measure-sans")
        .replace("$ERROR", "0");
    std::fs::write(&path, source).unwrap();
    let load = || {
        ModHost::load_with_grants(
            &path,
            ModGrants {
                hud: true,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let mut hosts = ModHosts::new(load(), Vec::new());
    let font = std::sync::Arc::new(assets::carriers::startup::diagnostic_font_catalog().unwrap());
    let metrics = std::sync::Arc::new(mod_ui::LabelMetrics::new(
        font,
        client_ui::test_support::text_metrics(
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
            Some(2),
        ),
    ));
    let observe = |hosts: &mut ModHosts| {
        hosts.observe_hud(
            None,
            Default::default(),
            Some(std::sync::Arc::clone(&metrics)),
        )
    };
    observe(&mut hosts);
    hosts.host_mut(0).frame(false).unwrap();
    *hosts.host_mut(0) = load();
    observe(&mut hosts);
    hosts.host_mut(0).frame(false).unwrap();
    assert!(hosts.host(0).is_active());
}

/// Control modes combine as movement intents do: a mode is on when any mod sets it, and a
/// trapped mod's modes are dropped with the rest of its output.
#[test]
fn control_modes_combine_across_mods_and_drop_a_trapped_mod() {
    let directory = tempfile::tempdir().unwrap();
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let set = [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (toggle_sprint, toggle_sneak, trap))| {
        let frame = format!(
            "i32.const 1 i32.const {} i32.const {} i32.const {} i32.const 256 call $modes {}",
            u8::from(toggle_sprint),
            u8::from(toggle_sneak),
            u8::from(trap),
            if trap { "unreachable" } else { "" }
        );
        let source = include_str!("../../../mod-host/src/tests/gameplay.wat")
            .replace("$GAMEPLAY", &format!("{name}/gameplay@{version}"))
            .replace("$INIT", "")
            .replace("$FRAME", &frame);
        let path = directory.path().join(format!("modes-{index}.wat"));
        std::fs::write(&path, source).unwrap();
        (
            path,
            ModGrants {
                movement: true,
                ..Default::default()
            },
        )
    })
    .collect();
    let mut runtime = load_set(set);
    let controls = ControlFrame {
        focused: true,
        ..mod_host::empty_controls()
    };
    let mut failures = Vec::new();
    let frame = |runtime: &mut ModHosts, failures: &mut Vec<usize>| {
        runtime.begin_frame();
        run_frame(
            runtime,
            FrameInput {
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
            |index, _| failures.push(index),
        )
    };
    let merged = frame(&mut runtime, &mut failures);
    assert_eq!(failures, [2]);
    assert_eq!(
        merged.control_modes,
        Some(GameplayControlModes {
            toggle_sprint: true,
            toggle_sneak: true,
            always_sprint: false,
        })
    );
    runtime.remove(1);
    let merged = frame(&mut runtime, &mut failures);
    assert_eq!(
        merged.control_modes,
        Some(GameplayControlModes {
            toggle_sprint: true,
            toggle_sneak: false,
            always_sprint: false,
        }),
        "an unloaded mod's modes are gone"
    );
    runtime.remove(0);
    assert_eq!(
        frame(&mut runtime, &mut failures).control_modes,
        None,
        "unloading the last mode owner restores the player's settings"
    );
}
