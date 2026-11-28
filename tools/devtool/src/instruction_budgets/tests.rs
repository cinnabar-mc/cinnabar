use super::*;

fn budgets(entries: &[(&str, u64)]) -> Budgets {
    Budgets {
        toolchain: "rustc 1.0.0".into(),
        budgets: entries
            .iter()
            .map(|(id, value)| ((*id).to_owned(), *value))
            .collect(),
    }
}

fn statuses(committed: &Budgets, measured: &Budgets) -> Vec<(String, Status)> {
    compare(committed, measured)
        .into_iter()
        .map(|row| (row.id, row.status))
        .collect()
}

#[test]
fn changes_inside_the_tolerance_keep_the_budget() {
    let committed = budgets(&[("a t", 100_000)]);
    for value in [99_000, 100_000, 101_000] {
        assert_eq!(
            statuses(&committed, &budgets(&[("a t", value)])),
            [("a t".to_owned(), Status::Within)]
        );
    }
}

#[test]
fn regressions_and_gains_past_the_tolerance_both_fail() {
    let committed = budgets(&[("a t", 100_000)]);
    assert_eq!(
        statuses(&committed, &budgets(&[("a t", 101_001)])),
        [("a t".to_owned(), Status::Over)]
    );
    assert_eq!(
        statuses(&committed, &budgets(&[("a t", 98_999)])),
        [("a t".to_owned(), Status::Under)]
    );
}

#[test]
fn new_and_removed_tests_are_reported() {
    let rows = statuses(&budgets(&[("a gone", 10)]), &budgets(&[("b new", 10)]));
    assert_eq!(
        rows,
        [
            ("a gone".to_owned(), Status::Stale),
            ("b new".to_owned(), Status::Unbudgeted)
        ]
    );
}

#[test]
fn refreshing_keeps_in_tolerance_budgets_and_replaces_the_rest() {
    let committed = budgets(&[("a kept", 100_000), ("b moved", 100_000), ("c gone", 10)]);
    let measured = budgets(&[("a kept", 100_800), ("b moved", 90_000), ("d new", 5)]);
    assert_eq!(
        refresh(&committed, &measured),
        budgets(&[("a kept", 100_000), ("b moved", 90_000), ("d new", 5)])
    );
    let mut new_toolchain = measured.clone();
    new_toolchain.toolchain = "rustc 2.0.0".into();
    assert_eq!(refresh(&committed, &new_toolchain), new_toolchain);
}

#[test]
fn report_explains_how_to_update_only_when_out_of_date() {
    let committed = budgets(&[("a t", 100)]);
    let current = report(&committed, &committed, &compare(&committed, &committed));
    assert!(!current.contains(UPDATE_HINT));
    let mut other_toolchain = budgets(&[("a t", 100)]);
    other_toolchain.toolchain = "rustc 2.0.0".into();
    let stale = report(
        &committed,
        &other_toolchain,
        &compare(&committed, &other_toolchain),
    );
    assert!(stale.contains("rustc 2.0.0"));
    assert!(stale.contains(UPDATE_HINT));
}

#[test]
fn reads_the_collected_total_from_a_callgrind_profile() {
    let profile = "version: 1\ncreator: callgrind-3.22.0\nevents: Ir\nfn=(1) work\n5 7\nsummary: 10902579\ntotals: 10902579\n";
    assert_eq!(callgrind_total(profile), Some(10_902_579));
    assert_eq!(callgrind_total("events: Ir\n"), None);
}

#[test]
fn listing_keeps_only_matching_tests_with_their_binaries() {
    let listing = r#"{
        "rust-suites": {
            "meshing::it": {
                "binary-path": "/target/debug/deps/it-1",
                "cwd": "/repo/crates/meshing",
                "testcases": {
                    "instruction_budgets::instruction_budget_liquids": {"filter-match": {"status": "matches"}},
                    "mesh::other": {"filter-match": {"status": "mismatch", "reason": "expression"}}
                }
            },
            "empty": {"binary-path": "/x", "cwd": "/y", "testcases": {}}
        }
    }"#;
    assert_eq!(
        parse_listing(listing).unwrap(),
        [BudgetTest {
            id: "meshing::it instruction_budgets::instruction_budget_liquids".into(),
            name: "instruction_budgets::instruction_budget_liquids".into(),
            binary: "/target/debug/deps/it-1".into(),
            cwd: "/repo/crates/meshing".into(),
        }]
    );
}

#[test]
fn committed_budgets_name_existing_budget_tests() {
    let text =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("instruction-budgets.json"))
            .unwrap();
    let committed: Budgets = serde_json::from_str(&text).unwrap();
    assert!(!committed.budgets.is_empty());
    for id in committed.budgets.keys() {
        let test = id.rsplit([':', ' ']).next().unwrap_or(id);
        assert!(
            test.starts_with(test_instructions::TEST_PREFIX),
            "{id} is not a budget test"
        );
    }
}
