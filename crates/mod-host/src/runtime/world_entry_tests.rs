use super::*;

/// Loads an inert component whose frame allowance can be inspected without guest imports.
fn instance(engine: &wasmtime::Engine) -> Instance {
    Instance::new(
        engine,
        br#"(component
        (core module $m (func (export "init")) (func (export "frame")))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#,
        ModGrants::default(),
        String::new(),
        Declared::default(),
        None,
    )
    .unwrap()
}

/// Supplies a host-owned world identity independently of the guest's observation grants.
fn observe(instance: &mut Instance, session: u64, dimension: i32) {
    instance
        .observe_presentation(
            Some(crate::WorldObservation {
                session,
                world: "budget-fixture".into(),
                dimension,
                position: (0., 64., 0.),
                alive: true,
                runtime_id: 1,
            }),
            None,
            None,
            Vec::new(),
        )
        .unwrap();
}

/// Reads the allowance actually armed in Wasmtime and completes the frame successfully.
fn frame_allowance(instance: &mut Instance) -> u64 {
    let mut armed = 0;
    assert_eq!(
        instance
            .run(FRAME_FUEL, commit, |_, store| {
                armed = store.get_fuel()?;
                Ok(())
            })
            .unwrap(),
        CallbackOutcome::Executed
    );
    armed
}

#[test]
fn first_observed_world_frame_has_a_single_larger_allowance() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut host = instance(&engine);
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
    observe(&mut host, 1, 0);
    assert_eq!(frame_allowance(&mut host), crate::WORLD_ENTRY_FRAME_FUEL);
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
}

#[test]
fn same_session_transitions_and_temporary_absence_do_not_rearm() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut host = instance(&engine);
    observe(&mut host, 1, 0);
    assert_eq!(frame_allowance(&mut host), crate::WORLD_ENTRY_FRAME_FUEL);
    observe(&mut host, 1, 1);
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
    host.clear_drawing_world();
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
    observe(&mut host, 1, 0);
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
    observe(&mut host, 2, 0);
    assert_eq!(frame_allowance(&mut host), crate::WORLD_ENTRY_FRAME_FUEL);
}

#[test]
fn pending_allowance_waits_for_a_current_world_and_valid_scope() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut host = instance(&engine);
    host.store.data_mut().drawing_policy = Some(Default::default());
    observe(&mut host, 1, 0);
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
    host.store
        .data_mut()
        .drawing_policy
        .as_mut()
        .unwrap()
        .connection = 1;
    observe(&mut host, 1, 0);
    host.observe_presentation(None, None, None, Vec::new())
        .unwrap();
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
    observe(&mut host, 1, 0);
    assert_eq!(frame_allowance(&mut host), crate::WORLD_ENTRY_FRAME_FUEL);
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
}

#[test]
fn invalid_observations_neither_arm_nor_replace_a_pending_session() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut host = instance(&engine);
    observe(&mut host, 1, 0);
    let mut invalid = host.store.data().player_state.world.clone().unwrap();
    invalid.session = 2;
    assert!(
        host.observe_presentation(Some(invalid), Some(f32::NAN), None, Vec::new())
            .is_err()
    );
    assert_eq!(frame_allowance(&mut host), crate::WORLD_ENTRY_FRAME_FUEL);
    assert!(
        host.observe_presentation(
            Some(crate::WorldObservation {
                session: 0,
                world: "invalid".into(),
                dimension: 0,
                position: (0., 0., 0.),
                alive: true,
                runtime_id: 1,
            }),
            None,
            None,
            Vec::new()
        )
        .is_err()
    );
    assert_eq!(frame_allowance(&mut host), FRAME_FUEL);
}

#[test]
fn busy_bonus_lane_runs_ordinary_work_and_preserves_the_pending_allowance() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut first = instance(&engine);
    let mut second = instance(&engine);
    let budget = crate::CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    first.callback_budget = Some(budget.clone());
    second.callback_budget = Some(budget.clone());
    observe(&mut first, 1, 0);
    observe(&mut second, 1, 0);
    assert_eq!(frame_allowance(&mut first), crate::WORLD_ENTRY_FRAME_FUEL);
    assert_eq!(frame_allowance(&mut second), FRAME_FUEL);
    budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    assert_eq!(frame_allowance(&mut first), FRAME_FUEL);
    assert_eq!(frame_allowance(&mut second), crate::WORLD_ENTRY_FRAME_FUEL);
    budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    assert_eq!(frame_allowance(&mut second), FRAME_FUEL);
}

#[test]
fn ordinary_pool_exhaustion_defers_without_spending_the_session_allowance() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut host = instance(&engine);
    let budget = crate::CallbackBudget::new(FRAME_FUEL - 1);
    host.callback_budget = Some(budget.clone());
    observe(&mut host, 1, 0);
    assert_eq!(
        host.run(FRAME_FUEL, commit, |_, _| panic!("must defer"))
            .unwrap(),
        CallbackOutcome::Deferred
    );
    budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    assert_eq!(frame_allowance(&mut host), crate::WORLD_ENTRY_FRAME_FUEL);
}

#[test]
fn unused_extra_fuel_never_becomes_regular_frame_capacity() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut host = instance(&engine);
    let budget = crate::CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    host.callback_budget = Some(budget.clone());
    observe(&mut host, 1, 0);
    let spent = FRAME_FUEL / 2;
    host.run(FRAME_FUEL, commit, |_, store| {
        store.set_fuel(crate::WORLD_ENTRY_FRAME_FUEL - spent)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(host.last_fuel, spent);
    assert_eq!(budget.remaining(), mod_api::TOTAL_MOD_FRAME_FUEL - spent);
}

#[test]
fn entry_callback_executes_work_above_the_ordinary_limit_without_refunding_extra() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let source = format!(
        r#"(component
        (core module $m
            (global $done (mut i32) (i32.const 0))
            (func (export "init"))
            (func (export "frame") (local $i i32)
                global.get $done if return end
                i32.const 1 global.set $done
                i32.const {} local.set $i
                (loop $work
                    local.get $i i32.const 1 i32.sub local.tee $i br_if $work)))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#,
        FRAME_FUEL / 2
    );
    let mut host = Instance::new(
        &engine,
        source.as_bytes(),
        ModGrants::default(),
        String::new(),
        Declared::default(),
        None,
    )
    .unwrap();
    let budget = crate::CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    host.callback_budget = Some(budget.clone());
    observe(&mut host, 1, 0);
    host.frame(CallbackOutcome::Executed).unwrap();
    assert!(host.last_fuel > FRAME_FUEL && host.last_fuel < crate::WORLD_ENTRY_FRAME_FUEL);
    assert_eq!(
        budget.remaining(),
        mod_api::TOTAL_MOD_FRAME_FUEL - FRAME_FUEL
    );
    budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    host.frame(CallbackOutcome::Executed).unwrap();
    assert!(host.last_fuel < FRAME_FUEL);
}

#[test]
fn boosted_fuel_trap_uses_normal_recovery_and_cannot_renew_the_bonus() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut host = Instance::new(
        &engine,
        br#"(component
        (core module $m (func (export "init"))
            (func (export "frame") (loop $forever br $forever)))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#,
        ModGrants::default(),
        String::new(),
        Declared::default(),
        None,
    )
    .unwrap();
    observe(&mut host, 1, 0);
    for attempt in 0..MAX_CONSECUTIVE_FRAME_OVERRUNS {
        let error = host.frame(CallbackOutcome::Executed).unwrap_err();
        assert_eq!(
            error.downcast_ref::<wasmtime::Trap>(),
            Some(&wasmtime::Trap::OutOfFuel)
        );
        assert_eq!(
            host.last_fuel,
            if attempt == 0 {
                crate::WORLD_ENTRY_FRAME_FUEL
            } else {
                FRAME_FUEL
            }
        );
        if attempt + 1 < MAX_CONSECUTIVE_FRAME_OVERRUNS {
            assert!(host.status == InstanceStatus::Recovering);
            assert!(host.recover_frame_fuel().unwrap());
            observe(&mut host, 1, 0);
        } else {
            assert!(host.status == InstanceStatus::Quarantined);
        }
    }
}

#[test]
fn hot_replacement_preserves_pending_and_consumed_session_allowances() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut host = instance(&engine);
    observe(&mut host, 1, 0);
    let component = wasmtime::component::Component::new(
        &engine,
        br#"(component
        (core module $m (func (export "init")) (func (export "frame")))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#,
    )
    .unwrap();
    for expected in [crate::WORLD_ENTRY_FRAME_FUEL, FRAME_FUEL] {
        host = host
            .replacement(
                &engine,
                &component,
                ModGrants::default(),
                String::new(),
                Declared::default(),
                None,
            )
            .unwrap();
        observe(&mut host, 1, 0);
        assert_eq!(frame_allowance(&mut host), expected);
    }
}
