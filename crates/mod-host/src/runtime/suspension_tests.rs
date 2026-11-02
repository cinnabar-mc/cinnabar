use super::*;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// Admits a real guest with deterministic scheduling time and no filesystem persistence.
fn scheduled(
    source: &str,
    step: u64,
) -> (
    Instance,
    crate::execution::EpochTicker,
    crate::CallbackBudget,
) {
    let (engine, ticker) = crate::execution::engine().unwrap();
    let mut instance = Instance::new(
        &engine,
        source.as_bytes(),
        ModGrants {
            controls: true,
            environment: true,
            ..Default::default()
        },
        String::new(),
        Declared::default(),
        None,
    )
    .unwrap();
    let mut budget = crate::CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    let now = AtomicU64::new(0);
    budget.time = crate::frame_time::FrameTime::fake(move || {
        Duration::from_nanos(now.fetch_add(step, Ordering::Relaxed))
    });
    next_frame(&mut instance, &budget);
    (instance, ticker, budget)
}

/// Advances the host boundary, without polling a continuation or sleeping.
fn next_frame(instance: &mut Instance, budget: &crate::CallbackBudget) {
    budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    budget.schedule_frames(&[instance.frame_owner()]);
    instance.begin_frame();
    instance.set_frame_authority(budget.clone(), false, false);
}

/// Finishes the original invocation, counting host frames without starting a second one.
fn finish(instance: &mut Instance, budget: &crate::CallbackBudget) -> Result<u64> {
    for _ in 0..256 {
        match instance.run_frame_slice()? {
            CallbackOutcome::Executed => return Ok(instance.last_fuel()),
            CallbackOutcome::Deferred | CallbackOutcome::Suspended => next_frame(instance, budget),
        }
    }
    panic!("bounded guest did not finish after its scheduling debt was repaid")
}

/// Spends several fuel yield intervals, retaining a global changed before the first yield.
fn resumable_source() -> String {
    crate::tests::fixture(
        r#"
        (local $left i32)
        global.get $progress i32.const 1 i32.add global.set $progress
        call $pressed i32.eqz if unreachable end
        i32.const 12000 local.set $left
        (loop $work local.get $left i32.const 1 i32.sub local.tee $left br_if $work)
        global.get $progress i32.const 1 i32.ne if unreachable end
        call $pressed i32.eqz if unreachable end
        i32.const 1 i32.const 4 i32.const 512 call $label
    "#,
        "Hello",
    )
    .replacen(
        "(func (export \"init\")",
        "(global $progress (mut i32) (i32.const 0))\n(func (export \"init\")",
        1,
    )
}

#[test]
fn guest_state_input_and_fuel_survive_different_yield_schedules() {
    let source = resumable_source();
    let mut fuel = Vec::new();
    for step in [0, 2_000_000] {
        let (mut instance, _ticker, budget) = scheduled(&source, step);
        instance.store.data_mut().pressed = true;
        if step != 0 {
            assert_eq!(
                instance.run_frame_slice().unwrap(),
                CallbackOutcome::Suspended
            );
            assert_eq!(instance.label(), Some("Hello"));
            assert_eq!(instance.consecutive_frame_overruns, 0);
            // A guest wake or a second delivery in the same host frame cannot repoll it.
            assert_eq!(
                instance.run_frame_slice().unwrap(),
                CallbackOutcome::Suspended
            );
            next_frame(&mut instance, &budget);
            instance.store.data_mut().pressed = false;
        }
        fuel.push(finish(&mut instance, &budget).unwrap());
        assert_eq!(instance.label(), Some("ello"));
        assert_eq!(instance.consecutive_frame_overruns, 0);
        assert!(instance.is_active());
    }
    assert_eq!(fuel[0], fuel[1]);
}

#[test]
fn output_waits_for_successful_post_return_and_its_trap_discards_every_write() {
    for trap in [false, true] {
        let source = crate::tests::fixture(
            "i32.const 1 i32.const 4 i32.const 512 call $label",
            "Hello",
        )
        .replacen(
            "(func (export \"frame\")",
            &format!(
                r#"
                (func (export "post") (local $left i32)
                    i32.const 12000 local.set $left
                    (loop $work local.get $left i32.const 1 i32.sub local.tee $left br_if $work)
                    {})
                (func (export "frame")"#,
                if trap { "unreachable" } else { "" }
            ),
            1,
        )
        .replace(
            "(canon lift (core func $run \"frame\"))",
            "(canon lift (core func $run \"frame\") (post-return (core func $run \"post\")))",
        );
        let (mut instance, _ticker, budget) = scheduled(&source, 2_000_000);
        assert_eq!(
            instance.run_frame_slice().unwrap(),
            CallbackOutcome::Suspended
        );
        assert_eq!(instance.label(), Some("Hello"));
        let result = finish(&mut instance, &budget);
        assert_eq!(result.is_err(), trap);
        assert_eq!(instance.label(), if trap { None } else { Some("ello") });
    }
}

#[test]
fn late_frame_actions_are_dropped_but_retained_output_commits() {
    let source = crate::tests::fixture(r#"
        (local $left i32)
        i32.const 1000 i32.const 512 call $delay
        i32.const 12000 local.set $left
        (loop $work local.get $left i32.const 1 i32.sub local.tee $left br_if $work)
        i32.const 1 i32.const 4 i32.const 512 call $label
    "#, "Hello")
        .replacen("(component", r#"(component
            (import "$GAMEPLAY" (instance $gameplay
                (export "set-packet-delay" (func (param "delay-ms" u32) (result (result (error string)))))))
            (alias export $gameplay "set-packet-delay" (func $delay))"#, 1)
        .replace("(core module $code", "(core func $lower-delay (canon lower (func $delay) (memory $memory) (realloc $realloc)))\n(core module $code\n(import \"host\" \"delay\" (func $delay (param i32 i32)))")
        .replace("(core instance $host", "(core instance $host (export \"delay\" (func $lower-delay))");
    let source = source.replace("$GAMEPLAY", &interface("gameplay"));
    let (mut instance, _ticker, budget) = scheduled(&source, 2_000_000);
    instance.store.data_mut().grants.packet_delay = true;
    assert_eq!(
        instance.run_frame_slice().unwrap(),
        CallbackOutcome::Suspended
    );
    next_frame(&mut instance, &budget);
    finish(&mut instance, &budget).unwrap();
    assert_eq!(instance.label(), Some("ello"));
    assert_eq!(instance.packet_delay_ms(), 0);
}

#[test]
fn world_session_and_revocation_cancel_without_an_overrun() {
    for transition in [0, 1, 2] {
        let (mut instance, _ticker, _) = scheduled(&resumable_source(), 2_000_000);
        instance.store.data_mut().pressed = true;
        assert_eq!(
            instance.run_frame_slice().unwrap(),
            CallbackOutcome::Suspended
        );
        match transition {
            0 => instance.clear_drawing_world(),
            1 => instance.set_session(Arc::new(SessionData::default())),
            _ => instance.revoke(),
        }
        assert!(instance.continuation.is_none());
        assert_eq!(instance.consecutive_frame_overruns, 0);
        assert_ne!(instance.status, InstanceStatus::Quarantined);
    }
}

#[test]
fn real_fuel_exhaustion_is_identical_across_schedules_and_three_overruns_quarantine() {
    let source = crate::tests::fixture("(loop $forever br $forever)", "Hello");
    for step in [0, 2_000_000] {
        let (mut instance, _ticker, budget) = scheduled(&source, step);
        for count in 1..=MAX_CONSECUTIVE_FRAME_OVERRUNS {
            let error = finish(&mut instance, &budget).unwrap_err();
            assert_eq!(
                error.downcast_ref::<wasmtime::Trap>(),
                Some(&wasmtime::Trap::OutOfFuel)
            );
            assert_eq!(instance.last_fuel(), FRAME_FUEL);
            assert_eq!(instance.consecutive_frame_overruns, count);
            if count < MAX_CONSECUTIVE_FRAME_OVERRUNS {
                next_frame(&mut instance, &budget);
                let previous = instance.frame_owner();
                assert!(instance.recover_frame_fuel().unwrap());
                assert_ne!(instance.frame_owner(), previous);
                budget.time.reset();
                budget.time.configure(&[previous]);
                assert!(budget.time.slice(previous).is_some());
                next_frame(&mut instance, &budget);
            }
        }
        assert!(!instance.is_active());
    }
}

#[test]
fn every_decision_export_keeps_its_instance_synchronous() {
    let fixtures = [
        include_str!("../tests/action_filter.wat")
            .replace("$BODY", "")
            .replace("$COUNT", "local.get $count"),
        include_str!("../tests/sound.wat")
            .replace("$BODY", "")
            .replace("$LOOPING", "local.get 6")
            .replace("$CANCELLED", "local.get 7"),
        include_str!("../tests/teleport.wat")
            .replace("$BODY", "")
            .replace("$YAW", "f32.const 0")
            .replace("$PITCH", "f32.const 0")
            .replace("$SERVER_YAW", "f32.const 0"),
    ];
    for source in fixtures {
        let source = source.replace(
            "(func (export \"frame\"))",
            r#"(func (export "frame")
            (local $left i32) i32.const 12000 local.set $left
            (loop $work local.get $left i32.const 1 i32.sub local.tee $left br_if $work))"#,
        );
        let (mut instance, _ticker, _) = scheduled(&source, 2_000_000);
        assert!(!instance.can_suspend());
        instance.frame(CallbackOutcome::Executed).unwrap();
        assert!(instance.continuation.is_none());
    }
}

/// Starts an on-disk guest so reload and event tests use the public host boundary.
fn suspended_host() -> (tempfile::TempDir, crate::ModHost, crate::CallbackBudget) {
    let (directory, mut host, budget) = scheduled_host(
        &resumable_source(),
        ModGrants {
            controls: true,
            ..Default::default()
        },
    );
    host.frame(true).unwrap();
    assert!(host.instance.continuation.is_some());
    (directory, host, budget)
}

/// Prepares a scheduled host, leaving declarations and input adjustable before guest entry.
fn scheduled_host(
    source: &str,
    grants: ModGrants,
) -> (tempfile::TempDir, crate::ModHost, crate::CallbackBudget) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("suspended.wat");
    std::fs::write(&path, source).unwrap();
    let mut host = crate::ModHost::load_with_grants(&path, grants).unwrap();
    let mut budget = crate::CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    let now = AtomicU64::new(0);
    budget.time = crate::frame_time::FrameTime::fake(move || {
        Duration::from_nanos(now.fetch_add(2_000_000, Ordering::Relaxed))
    });
    next_frame(&mut host.instance, &budget);
    (directory, host, budget)
}

#[test]
fn events_coalesce_with_a_bound_independent_of_suspension_duration() {
    let (_directory, mut host, _) = suspended_host();
    host.instance
        .store
        .data_mut()
        .declared
        .actions
        .insert("edit".into());
    for index in 0..1024 {
        let outcome = host
            .dispatch(vec![
                ModEvent::DataChanged(vec![crate::DataSource::Items, crate::DataSource::Recipes]),
                ModEvent::TextChanged {
                    control: "edit".into(),
                    text: index.to_string(),
                },
                ModEvent::Action {
                    id: "edit".into(),
                    index: None,
                },
                ModEvent::Scrolled {
                    delta: 1.,
                    x: 0.,
                    y: 0.,
                    modifiers: Default::default(),
                },
            ])
            .unwrap();
        assert_eq!(outcome, CallbackOutcome::Suspended);
        // One revision event and one current value for the only declared edit control.
        assert_eq!(host.pending_events.len(), 2);
        assert!(
            matches!(&host.pending_events[1], ModEvent::TextChanged { text, .. } if text == &index.to_string())
        );
        assert_eq!(host.instance.consecutive_frame_overruns, 0);
    }
    assert!(host.instance.continuation.is_some());
}

#[test]
fn reload_failure_keeps_the_old_continuation_and_success_replaces_it_atomically() {
    let (directory, mut host, budget) = suspended_host();
    let previous = host.frame_owner();
    let path = directory.path().join("suspended.wat");
    let failed = crate::tests::fixture("", "Candidate").replace(
        "i32.const 0 i32.const 9 i32.const 512 call $label",
        "unreachable",
    );
    std::fs::write(&path, failed).unwrap();
    assert!(host.reload_if_changed().is_err());
    assert_eq!(host.frame_owner(), previous);
    assert!(host.instance.continuation.is_some());
    assert_eq!(host.label(), Some("Hello"));
    assert_eq!(host.instance.consecutive_frame_overruns, 0);
    next_frame(&mut host.instance, &budget);
    std::fs::write(&path, crate::tests::fixture("", "Replacement")).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_ne!(host.frame_owner(), previous);
    budget.time.reset();
    budget.time.configure(&[previous]);
    assert!(budget.time.slice(previous).is_some());
    assert!(host.instance.continuation.is_none());
    assert_eq!(host.label(), Some("Replacement"));
    assert_eq!(host.instance.consecutive_frame_overruns, 0);
}

/// Records continuation destruction without starting any process or background task.
struct DropProbe(Arc<AtomicU64>);
impl Drop for DropProbe {
    /// Marks the owned future as cancelled when its enclosing mod is unloaded.
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn unload_drops_the_owned_continuation_without_recording_a_fault() {
    let (directory, mut host, budget) = suspended_host();
    let previous = host.frame_owner();
    let drops = Arc::new(AtomicU64::new(0));
    let probe = DropProbe(drops.clone());
    let call = host.instance.continuation.as_mut().unwrap();
    let future = call.future.get_mut().unwrap();
    let previous_future = std::mem::replace(future, Box::pin(std::future::pending()));
    *future = Box::pin(async move {
        let _probe = probe;
        previous_future.await
    });
    assert_eq!(host.instance.consecutive_frame_overruns, 0);
    drop(host);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    budget.time.reset();
    budget.time.configure(&[previous]);
    assert!(budget.time.slice(previous).is_some());
    drop(directory);
}

#[test]
fn cancelling_or_replacing_a_suspended_world_entry_releases_unspent_fuel() {
    for transition in 0..3 {
        let (directory, mut host, budget) = scheduled_host(
            &resumable_source(),
            ModGrants {
                controls: true,
                ..Default::default()
            },
        );
        host.observe_presentation(
            Some(crate::WorldObservation {
                session: 1,
                world: "reservation-fixture".into(),
                dimension: 0,
                position: (0., 64., 0.),
                alive: true,
                runtime_id: 1,
            }),
            None,
            None,
            Vec::new(),
        )
        .unwrap();
        host.frame(true).unwrap();
        assert!(host.instance.continuation.is_some());
        assert_eq!(budget.remaining(), 0);
        match transition {
            0 => drop(host),
            1 => {
                std::fs::write(
                    directory.path().join("suspended.wat"),
                    crate::tests::fixture("", "Replacement"),
                )
                .unwrap();
                assert!(host.reload_if_changed().unwrap());
                assert!(host.instance.continuation.is_none());
            }
            _ => {
                host.instance.clear_drawing_world();
                assert!(host.instance.continuation.is_none());
                assert_eq!(host.instance.consecutive_frame_overruns, 0);
            }
        }
        assert!(budget.remaining() > 0);
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        assert_eq!(budget.remaining(), mod_api::TOTAL_MOD_FRAME_FUEL);
    }
}

#[test]
fn clock_and_import_counters_are_invocation_scoped() {
    use cinnabar::extension::{clock::Host as _, input::Host as _};
    let mut state = State::new(
        ModGrants {
            clock: true,
            controls: true,
            ..Default::default()
        },
        String::new(),
        Declared::default(),
    );
    state.begin_callback();
    let clock = state.read_local_time().unwrap().unwrap();
    let mut transaction = state.frame_transaction();
    state.clock.as_mut().unwrap().unix_ms += 1;
    assert_eq!(transaction.read_local_time().unwrap().unwrap(), clock);
    for _ in 0..MAX_IMPORT_WRITES {
        transaction.read_controls().unwrap().unwrap();
    }
    assert!(transaction.read_controls().is_err());
}

#[test]
fn time_admission_defers_without_starting_or_resetting_the_guest() {
    let source = crate::tests::fixture("unreachable", "Hello");
    let (mut instance, _ticker, budget) = scheduled(&source, 0);
    budget
        .time
        .charge(instance.frame_owner(), Duration::from_millis(1));
    assert_eq!(
        instance.run_frame_slice().unwrap(),
        CallbackOutcome::Deferred
    );
    assert!(instance.continuation.is_none());
    assert_eq!(instance.label(), Some("Hello"));
    assert_eq!(instance.consecutive_frame_overruns, 0);
    assert_eq!(instance.status, InstanceStatus::Running);
}

#[test]
fn staged_settings_are_readable_only_inside_the_invocation() {
    use cinnabar::extension::settings::Host as _;
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut native = State::new(
        ModGrants {
            settings: true,
            ..Default::default()
        },
        "{}".into(),
        Declared::default(),
    );
    let mut transaction = native.frame_transaction();
    transaction.begin_callback();
    let saved = r#"{"choice":true}"#.to_owned();
    transaction.save(saved.clone()).unwrap().unwrap();
    assert_eq!(transaction.load().unwrap().unwrap(), saved);
    assert_eq!(native.load().unwrap().unwrap(), "{}");
    native.accept_frame(transaction, false, true, 0);
    let mut store = Store::new(&engine, native);
    commit_frame_outputs(&mut store, true);
    assert_eq!(store.data_mut().load().unwrap().unwrap(), saved);
}

#[test]
fn native_edit_queues_coalesce_values_and_drop_keys_while_suspended() {
    use ui::mod_panel::{FieldEvent, FieldKey, FieldModifiers, TextEvent};
    let (_directory, mut host, _) = suspended_host();
    native_panel(&mut host);
    for index in 0..1024 {
        let text = TextEvent {
            id: "name".into(),
            value: index.to_string(),
        };
        host.queue_text_events(vec![text.clone()]).unwrap();
        host.queue_field_events(vec![
            FieldEvent::Change(text.clone()),
            FieldEvent::Submit(text),
            FieldEvent::Key(FieldKey {
                id: "name".into(),
                key: "Enter".into(),
                modifiers: FieldModifiers::default(),
            }),
        ])
        .unwrap();
        let controls = &host.instance.store.data().controls;
        assert_eq!(controls.text_events.len(), 1);
        assert_eq!(controls.text_events[0].value, index.to_string());
        assert!(
            matches!(controls.field_events.as_slice(), [FieldEvent::Change(event)] if event.value == index.to_string())
        );
    }
    assert!(host.instance.continuation.is_some());
    assert_eq!(host.instance.consecutive_frame_overruns, 0);
}

/// Installs one editable field so queue tests share the same validated native schema.
fn native_panel(host: &mut crate::ModHost) {
    host.set_panel_bundle(
        &serde_json::json!({
            "panel": {"surface": crate::tests::panel_surface("native"), "controls": [{
                "kind":"text", "id":"name", "label":"Name", "value":"Main",
                "min_chars":1, "max_chars":8, "live":true, "navigation_keys":true
            }]}, "surfaces":{}
        })
        .to_string(),
    )
    .unwrap();
}

/// Derives interface identities from the WIT package instead of repeating its version.
fn interface(name: &str) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (package, version) = package.split_once('@').unwrap();
    format!("{package}/{name}@{version}")
}

/// Completes the current continuation through the public frame and lifecycle path.
fn finish_host(host: &mut crate::ModHost, budget: &crate::CallbackBudget) {
    for _ in 0..256 {
        if host.instance.continuation.is_none() {
            return;
        }
        next_frame(&mut host.instance, budget);
        host.frame(false).unwrap();
    }
    panic!("bounded guest did not finish through the public frame path")
}

/// Asserts the same thread-safety bounds required by the client's resource owner.
fn resource_traits<T: Send + Sync>() {}

#[test]
fn suspended_mod_hosts_keep_the_client_resource_thread_safety_contract() {
    resource_traits::<crate::ModHost>();
}

#[test]
fn completing_an_old_frame_keeps_new_native_edits_for_the_next_invocation() {
    use ui::mod_panel::{FieldEvent, TextEvent};
    let source = crate::tests::fixture(
        r#"(local $left i32)
        i32.const 12000 local.set $left
        (loop $work local.get $left i32.const 1 i32.sub local.tee $left br_if $work)"#,
        "Hello",
    );
    let (_directory, mut host, budget) = scheduled_host(
        &source,
        ModGrants {
            controls: true,
            ..Default::default()
        },
    );
    native_panel(&mut host);
    host.frame(false).unwrap();
    assert!(host.instance.continuation.is_some());
    let edit = TextEvent {
        id: "name".into(),
        value: "Updated".into(),
    };
    host.queue_text_events(vec![edit.clone()]).unwrap();
    host.queue_field_events(vec![FieldEvent::Change(edit)])
        .unwrap();
    finish_host(&mut host, &budget);
    assert_eq!(
        host.instance.store.data().controls.text_events[0].value,
        "Updated"
    );
    assert!(
        matches!(host.instance.store.data().controls.field_events.as_slice(), [FieldEvent::Change(edit)] if edit.value == "Updated")
    );
    let received = Arc::new(AtomicU64::new(0));
    let observed = received.clone();
    host.instance.store.call_hook(move |store, hook| {
        if matches!(hook, wasmtime::CallHook::CallingWasm) {
            let controls = &store.data().controls;
            if controls
                .text_events
                .iter()
                .any(|edit| edit.value == "Updated")
                && controls.field_events.iter().any(
                    |event| matches!(event, FieldEvent::Change(edit) if edit.value == "Updated"),
                )
            {
                observed.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    });
    for _ in 0..256 {
        next_frame(&mut host.instance, &budget);
        host.frame(false).unwrap();
        if received.load(Ordering::Relaxed) != 0 {
            break;
        }
    }
    assert_ne!(received.load(Ordering::Relaxed), 0);
    assert!(host.instance.store.data().controls.text_events.is_empty());
    assert!(host.instance.store.data().controls.field_events.is_empty());
}

#[test]
fn screen_text_edits_fence_stale_publication_even_behind_a_lifecycle_event() {
    let source = crate::tests::fixture(r#"(local $left i32)
        i32.const 128 i32.const 9 i32.const 256 i32.const 5 i32.const 512 call $text
        i32.const 512 i32.load8_u if unreachable end
        i32.const 12000 local.set $left
        (loop $work local.get $left i32.const 1 i32.sub local.tee $left br_if $work)"#, "Hello")
        .replacen("(component", r#"(component
            (import "$SCREEN" (instance $screen
                (export "set-text" (func (param "control" string) (param "text" string) (result (result (error string)))))))
            (alias export $screen "set-text" (func $text))"#, 1)
        .replacen("(data (i32.const 0)", "(data (i32.const 128) \"demo.edit\")\n(data (i32.const 256) \"stale\")\n(data (i32.const 0)", 1)
        .replace("(core module $code", "(core func $lower-text (canon lower (func $text) (memory $memory) (realloc $realloc)))\n(core module $code\n(import \"host\" \"text\" (func $text (param i32 i32 i32 i32 i32)))")
        .replace("(core instance $host", "(core instance $host (export \"text\" (func $lower-text))")
        .replace("$SCREEN", &interface("screen"));
    let (_directory, mut host, budget) = scheduled_host(
        &source,
        ModGrants {
            screen: true,
            ..Default::default()
        },
    );
    host.instance
        .store
        .data_mut()
        .declared
        .actions
        .insert("demo.edit".into());
    host.frame(false).unwrap();
    assert!(host.instance.continuation.is_some());
    host.dispatch(vec![
        ModEvent::DataChanged(vec![crate::DataSource::Items]),
        ModEvent::TextChanged {
            control: "demo.edit".into(),
            text: "new native value".into(),
        },
    ])
    .unwrap();
    finish_host(&mut host, &budget);
    assert!(!host.screens().data.texts.contains_key("demo.edit"));
    assert!(host.pending_events.iter().any(
        |event| matches!(event, ModEvent::TextChanged { text, .. } if text == "new native value")
    ));
}

#[path = "suspension_tests/drawing.rs"]
mod drawing;
