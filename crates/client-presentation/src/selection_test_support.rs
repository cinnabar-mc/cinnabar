//! Original predicates for cold and cached equipment-selection regressions.

/// Operations in one cold predicate, so two rejected mappings exhaust selection.
pub(crate) const REJECTED_PREDICATE_OPS: usize = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK / 2;

/// Builds a false predicate that fills half the selection budget with variable reads.
pub(crate) fn rejected_predicate(index: usize) -> String {
    let variable = format!("v.fixture_rejected_{index}");
    // Each sum statement takes eight operations; the final statement and result take eight.
    let statement = format!("{variable}+{variable}+{variable}+{variable};");
    format!(
        "{}!({variable}+{variable}+{variable});",
        statement.repeat((REJECTED_PREDICATE_OPS - 8) / 8),
    )
}

#[test]
fn rejected_candidates_fill_the_cold_runtime_budget() {
    let budget = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK;
    let count = budget / REJECTED_PREDICATE_OPS;
    assert!(count > 0);
    let cost: usize = (0..count)
        .map(|index| {
            let program =
                pack_entity_compiler::compile_molang_expression(&rejected_predicate(index))
                    .unwrap();
            usize::from(program.expressions[0].op_count)
        })
        .sum();
    assert_eq!(cost, budget);
}
