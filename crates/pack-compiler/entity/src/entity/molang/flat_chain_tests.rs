use crate::compile_molang_expression;

#[test]
fn flat_boolean_source_byte_boundary_is_preserved() {
    let mut source = String::from("variable.a || variable.b");
    source.extend(std::iter::repeat_n(
        ' ',
        assets::MAX_ENTITY_SOURCE_BYTES - source.len(),
    ));
    compile_molang_expression(&source).expect("the existing source byte budget is inclusive");
    source.push(' ');
    let error =
        compile_molang_expression(&source).expect_err("the source byte guard still applies");
    assert!(
        error
            .to_string()
            .contains("Molang source size exceeds bound")
    );
}

#[test]
fn flat_boolean_operation_budget_is_preserved() {
    let limit = assets::MAX_MOLANG_OPS_PER_EXPRESSION;
    for operator in ["||", "&&"] {
        let chain = |terms| {
            (0..terms)
                .map(|index| format!("variable.input_{index}"))
                .collect::<Vec<_>>()
                .join(operator)
        };
        // Dynamic operands need a read and a branch; leave room for the boolean join.
        let program = compile_molang_expression(&chain(limit / 2 - 2))
            .expect("a shallow chain near the operation budget is admitted");
        assert!(program.ops.len() > limit / 2);
        assert!(program.ops.len() <= limit);
        let error = compile_molang_expression(&chain(limit + 1))
            .expect_err("flattening does not remove the existing operation budget");
        assert!(
            error
                .to_string()
                .contains("Molang operation count exceeds bound")
        );
    }
}

#[test]
fn flat_boolean_chains_remain_within_source_and_operation_budgets() {
    for operator in ["||", "&&"] {
        let chain = (0..80)
            .map(|index| format!("variable.input == {index}"))
            .collect::<Vec<_>>()
            .join(&format!(" {operator} "));
        let source = format!("variable.result = {chain};");
        let result = compile_molang_expression(&source);
        assert!(
            result.is_ok(),
            "a flat {operator} chain is not nested input: {result:?}"
        );
    }
}
