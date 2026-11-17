use std::fs;

use super::*;
use crate::auth::test_support::{event_child, event_child_holding};

/// The core's ownership events, one per line (core/authflow/testdata).
const FIXTURE: &str = include_str!("../../../../core/authflow/testdata/ownership_events.jsonl");

fn run(lines: &[&str]) -> (Finding, Vec<(String, String)>) {
    let (child, directory) = event_child(lines);
    let mut codes = Vec::new();
    let finding = follow(child.into(), &AtomicBool::new(false), |uri, code| {
        codes.push((uri.to_owned(), code.to_owned()));
    });
    let _ = fs::remove_dir_all(directory);
    (finding, codes)
}

#[test]
fn every_core_ownership_event_parses() {
    let results: Vec<_> = FIXTURE
        .lines()
        .map(|line| serde_json::from_str::<WireEvent>(line).unwrap())
        .filter_map(|event| match event {
            WireEvent::Ownership { result, .. } => Some(result),
            _ => None,
        })
        .collect();
    assert_eq!(
        results,
        [
            Check::Verified,
            Check::NotOwned,
            Check::SignInRequired,
            Check::Unverifiable
        ]
    );
}

#[test]
fn a_sign_in_shows_its_code_and_reports_a_new_account() {
    let lines: Vec<&str> = FIXTURE.lines().take(3).collect();
    let (finding, codes) = run(&lines);
    assert_eq!(
        finding,
        Finding {
            check: Check::Verified,
            reason: "entitled".into(),
            account: "6f1c2a9e-0000-4000-8000-000000000001".into()
        }
    );
    assert_eq!(
        codes,
        [(
            "https://login.example.test/device".to_owned(),
            "ABCD-1234".to_owned()
        )]
    );
}

#[test]
fn a_stored_account_check_is_not_a_new_sign_in() {
    let (finding, codes) = run(&[
        r#"{"v":1,"event":"checking_cache"}"#,
        r#"{"v":1,"event":"ownership","result":"not_owned","reason":"no_minecraft_entitlement"}"#,
    ]);
    assert_eq!(finding.check, Check::NotOwned);
    assert!(codes.is_empty());
}

/// A shown code alone replaces nothing; the answer names the sign-in still stored.
#[test]
fn an_unfinished_sign_in_keeps_the_old_account() {
    let (finding, codes) = run(&[
        r#"{"v":1,"event":"checking_cache"}"#,
        r#"{"v":1,"event":"device_code","verification_uri":"https://login.example.test/device","user_code":"ABCD-1234"}"#,
        r#"{"v":1,"event":"ownership","result":"sign_in_required","reason":"device_complete:http_400:access_denied","account":"old"}"#,
    ]);
    assert_eq!(codes.len(), 1);
    assert_eq!(
        (finding.check, finding.account.as_str()),
        (Check::SignInRequired, "old")
    );
}

#[test]
fn a_helper_that_ends_or_misbehaves_is_unverifiable() {
    for lines in [
        &[r#"{"v":1,"event":"checking_cache"}"#][..],
        &[r#"{"v":1,"event":"ownership","result":"owned","reason":"x"}"#],
        &[r#"{"v":1,"event":"ownership","result":"verified","reason":"x","token":"t"}"#],
        &[r#"{"v":2,"event":"ownership","result":"verified","reason":"x"}"#],
        &[
            r#"{"v":1,"event":"device_code","verification_uri":"http://insecure.test","user_code":"A"}"#,
        ],
    ] {
        assert_eq!(run(lines).0.check, Check::Unverifiable, "{lines:?}");
    }
}

#[test]
fn cancelling_stops_a_waiting_helper() {
    let (child, directory) = event_child_holding(&[r#"{"v":1,"event":"checking_cache"}"#]);
    let finding = follow(child.into(), &AtomicBool::new(true), |_, _| {});
    let _ = fs::remove_dir_all(directory);
    assert_eq!(finding, Finding::unverifiable("cancelled"));
}

#[test]
fn reasons_are_short_printable_codes() {
    assert_eq!(
        log_safe("refresh:http_400:invalid_grant"),
        "refresh:http_400:invalid_grant"
    );
    assert_eq!(log_safe("a b\nc"), "abc");
    assert_eq!(log_safe("\n"), "");
    assert_eq!(log_safe(&"x".repeat(100)).len(), 64);
}
