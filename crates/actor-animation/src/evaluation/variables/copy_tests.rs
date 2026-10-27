//! Transactional copies preserve independent values while reusing sparse storage.

use super::*;

#[test]
fn copying_sparse_variables_visits_only_assigned_slots() {
    let layout = VariableLayout {
        variable_count: 100_000,
        temp_count: 40_000,
        ..Default::default()
    };
    let mut variables = layout.fresh(7);
    variables.set(Some(3), 5.0);
    variables
        .store(Place::Temporary(4), MolangValue::ActorReference(42))
        .unwrap();
    super::super::selection_work::take();
    let copy = variables.clone();
    let work = super::super::selection_work::take();
    assert!(work.copied <= 4, "copied unused authored slots: {work:?}");
    assert_eq!(copy.get(Some(3)), Some(5.0));
    assert_eq!(copy.get(Some(99_999)), None);
    assert_eq!(
        copy.read(Place::Temporary(4)),
        Some(MolangValue::ActorReference(42))
    );
    assert_eq!(copy.read(Place::Temporary(39_999)), None);
}

#[test]
fn warm_transactional_copies_allocate_nothing_and_remain_independent() {
    let layout = VariableLayout {
        variable_count: 8_192,
        temp_count: 4_096,
        ..Default::default()
    };
    let mut variables = layout.fresh(7);
    variables.set_string(Some(23), "owner");
    variables.set_string(Some(4_097), "released");
    variables.clear(Some(4_097));
    variables
        .store(
            Place::Temporary(6),
            MolangValue::String(Arc::from("temporary")),
        )
        .unwrap();
    variables.capture_writes();
    variables
        .store(Place::Variable(1), MolangValue::Number(12.0))
        .unwrap();
    variables.next_random();
    let retained = variables.clone();
    for _ in 0..3 {
        drop(variables.clone());
    }
    let (mut copy, allocated) = test_alloc::measure(|| variables.clone());
    assert_eq!(allocated, test_alloc::Allocations::default());
    assert_eq!(copy.variable(23), variables.variable(23));
    assert_eq!(
        copy.read(Place::Temporary(6)),
        variables.read(Place::Temporary(6))
    );
    assert_eq!(copy.random_state(), variables.random_state());
    assert_eq!(copy.random_draws, variables.random_draws);
    let mut published = layout.fresh(11);
    copy.publish_writes(&mut published);
    assert_eq!(published.get(Some(1)), Some(12.0));
    assert_eq!(published.variable(23), None);
    copy.set(Some(23), 9.0);
    copy.clear_temporaries();
    copy.next_random();
    assert_eq!(
        retained.variable(23),
        Some(MolangValue::String(Arc::from("owner")))
    );
    assert_eq!(variables.variable(23), retained.variable(23));
    assert_eq!(
        variables.read(Place::Temporary(6)),
        retained.read(Place::Temporary(6))
    );
    assert_eq!(variables.random_state(), retained.random_state());
}

#[test]
fn copying_a_changed_layout_clears_old_slots_and_preserves_string_ownership() {
    let small = VariableLayout {
        variable_count: 2,
        temp_count: 2,
        ..Default::default()
    };
    let large = VariableLayout {
        variable_count: 5,
        temp_count: 8,
        ..Default::default()
    };
    let mut source = small.fresh(3);
    source.set_string(Some(1), "source");
    source
        .store(Place::Temporary(1), MolangValue::Number(2.0))
        .unwrap();
    let mut copy = large.fresh(5);
    copy.store(Place::Temporary(7), MolangValue::String(Arc::from("old")))
        .unwrap();
    copy.set(Some(4), 8.0);
    copy.clone_from(&source);
    assert_eq!(copy.variable(1), source.variable(1));
    assert_eq!(
        copy.read(Place::Temporary(1)),
        Some(MolangValue::Number(2.0))
    );
    assert_eq!(copy.variable(4), None);
    assert_eq!(copy.read(Place::Temporary(7)), None);
    let mut next = large.fresh(9);
    next.store(Place::Temporary(7), MolangValue::ActorReference(42))
        .unwrap();
    copy.clone_from(&next);
    assert_eq!(copy.variable(1), None);
    assert_eq!(copy.read(Place::Temporary(1)), None);
    assert_eq!(
        copy.read(Place::Temporary(7)),
        Some(MolangValue::ActorReference(42))
    );
}
