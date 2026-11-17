use super::*;

const DAY: u64 = 24 * 60 * 60;
const T0: u64 = 1_800_000_000;

fn pass(verified_at: u64) -> Pass {
    Pass {
        verified_at,
        not_owned_since: None,
        latest_clock: verified_at,
        sign_in: String::new(),
    }
}

/// A temporary install's license cache and pass paths.
fn license_files() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("license/microsoft-token.json");
    let pass_file = dir.path().join("license/ownership.json");
    (dir, cache, pass_file)
}

#[test]
fn the_gate_applies_only_where_some_source_exists() {
    let none = Sources::default();
    assert!(!none.gated());
    assert!(
        Sources {
            local_install: true,
            account: false
        }
        .gated()
    );
    assert!(
        Sources {
            local_install: false,
            account: true
        }
        .gated()
    );
    assert_eq!(
        sources().local_install,
        cfg!(windows),
        "only Windows can find a store install"
    );
    assert_eq!(
        Sources::for_build(false).gated(),
        MSA_CLIENT_ID.is_some(),
        "without a local source, only a built-in client ID turns the gate on"
    );
}

#[test]
fn a_pass_plays_for_thirty_days_then_needs_a_check() {
    assert!(!pass_valid(None, T0));
    assert!(pass_valid(Some(&pass(T0)), T0));
    assert!(pass_valid(Some(&pass(T0)), T0 + PASS_LIFETIME_SECS - 1));
    assert!(!pass_valid(Some(&pass(T0)), T0 + PASS_LIFETIME_SECS));
}

#[test]
fn a_clock_set_back_or_a_long_gap_forces_a_check() {
    let seen = Pass {
        latest_clock: T0 + 5 * DAY,
        ..pass(T0)
    };
    assert!(pass_valid(Some(&seen), T0 + 5 * DAY - 60));
    assert!(!pass_valid(Some(&seen), T0 + 2 * DAY));
    assert!(!pass_valid(Some(&pass(T0)), T0 - DAY));
}

#[test]
fn verifying_after_the_clock_moves_back_restores_fast_starts() {
    let seen = Pass {
        latest_clock: T0 + 5 * DAY,
        ..pass(T0)
    };
    let (next, _) = record(Some(seen), Check::Verified, "", T0 + DAY);
    assert!(pass_valid(next.as_ref(), T0 + DAY + 60));
}

#[test]
fn a_clock_problem_never_blocks_by_itself() {
    let seen = Pass {
        latest_clock: T0 + 5 * DAY,
        ..pass(T0)
    };
    let now = T0 + DAY;
    assert!(!pass_valid(Some(&seen), now));
    assert_eq!(
        record(Some(seen), Check::Unverifiable, "", now).1,
        Verdict::Allow
    );
}

#[test]
fn verification_starts_a_pass_and_clears_a_pending_revocation() {
    let pending = Pass {
        not_owned_since: Some(T0 + DAY),
        latest_clock: T0 + DAY,
        ..pass(T0)
    };
    let (next, verdict) = record(Some(pending), Check::Verified, "", T0 + 2 * DAY);
    assert_eq!(verdict, Verdict::Allow);
    assert_eq!(next, Some(pass(T0 + 2 * DAY)));
    assert_eq!(record(None, Check::Verified, "", T0).0, Some(pass(T0)));
}

#[test]
fn failures_never_extend_a_pass() {
    for check in [Check::Unverifiable, Check::SignInRequired, Check::NotOwned] {
        let (next, _) = record(Some(pass(T0)), check, "", T0 + DAY);
        assert_eq!(next.map(|pass| pass.verified_at), Some(T0), "{check:?}");
    }
}

#[test]
fn a_prior_pass_survives_unverifiable_checks_even_after_it_expires() {
    let later = T0 + PASS_LIFETIME_SECS * 3;
    let (next, verdict) = record(Some(pass(T0)), Check::Unverifiable, "", later);
    assert_eq!(verdict, Verdict::Allow);
    assert_eq!(next.map(|pass| pass.latest_clock), Some(later));
}

#[test]
fn without_a_pass_every_failure_denies() {
    for check in [Check::NotOwned, Check::SignInRequired, Check::Unverifiable] {
        assert_eq!(record(None, check, "", T0), (None, Verdict::Deny(check)));
    }
}

#[test]
fn sign_in_is_required_only_once_the_pass_expires() {
    assert_eq!(
        record(Some(pass(T0)), Check::SignInRequired, "", T0 + DAY).1,
        Verdict::Allow
    );
    let (next, verdict) = record(
        Some(pass(T0)),
        Check::SignInRequired,
        "",
        T0 + PASS_LIFETIME_SECS,
    );
    assert_eq!(verdict, Verdict::Deny(Check::SignInRequired));
    assert!(
        next.is_some(),
        "a later unverifiable check still finds the pass"
    );
}

#[test]
fn revocation_needs_two_not_owned_answers_a_day_apart() {
    let (first, verdict) = record(Some(pass(T0)), Check::NotOwned, "", T0 + DAY);
    assert_eq!(verdict, Verdict::Allow);
    let (second, verdict) = record(first, Check::NotOwned, "", T0 + 2 * DAY - 1);
    assert_eq!(verdict, Verdict::Allow);
    assert_eq!(
        second.as_ref().and_then(|pass| pass.not_owned_since),
        Some(T0 + DAY),
        "a repeat keeps the first answer's time"
    );
    assert_eq!(
        record(second, Check::NotOwned, "", T0 + 2 * DAY),
        (None, Verdict::Deny(Check::NotOwned))
    );
}

#[test]
fn an_unverifiable_check_keeps_a_pending_revocation() {
    let (first, _) = record(Some(pass(T0)), Check::NotOwned, "", T0 + DAY);
    let (kept, _) = record(first, Check::Unverifiable, "", T0 + DAY + 60);
    assert_eq!(
        record(kept, Check::NotOwned, "", T0 + 2 * DAY).1,
        Verdict::Deny(Check::NotOwned)
    );
}

#[test]
fn a_new_sign_in_replaces_the_old_accounts_pass() {
    assert_eq!(
        record(Some(pass(T0)), Check::NotOwned, "b", T0 + DAY),
        (None, Verdict::Deny(Check::NotOwned))
    );
    assert_eq!(
        record(Some(pass(T0)), Check::Unverifiable, "b", T0 + DAY),
        (None, Verdict::Deny(Check::Unverifiable))
    );
}

#[test]
fn passes_round_trip_and_forgetting_the_license_removes_its_files() {
    let (dir, cache, pass_file) = license_files();
    assert_eq!(load_pass(&pass_file), None);
    save_pass(&pass_file, Some(&pass(T0))).unwrap();
    assert_eq!(load_pass(&pass_file), Some(pass(T0)));
    let sibling = dir.path().join("license/microsoft-token.json.entitlements");
    let other = dir.path().join("license/unrelated.json");
    for file in [&cache, &sibling, &other] {
        std::fs::write(file, b"{}").unwrap();
    }
    forget_license(&cache, &pass_file).unwrap();
    assert!(!cache.exists() && !sibling.exists());
    assert!(other.exists());
    assert_eq!(load_pass(&pass_file), None);
}

#[test]
fn a_check_finishing_after_removal_restores_no_pass() {
    let (_dir, cache, pass_file) = license_files();
    let lock = lock_checks(&cache).unwrap();
    let (verdict, saved) = record_check(&lock, &cache, &pass_file, Check::Verified, "a", T0);
    assert_eq!(verdict, Verdict::Allow);
    saved.unwrap();
    assert_eq!(load_pass(&pass_file), None);
    std::fs::write(&cache, b"{}").unwrap();
    record_check(&lock, &cache, &pass_file, Check::Verified, "a", T0)
        .1
        .unwrap();
    assert_eq!(
        load_pass(&pass_file),
        Some(Pass {
            sign_in: "a".into(),
            ..pass(T0)
        })
    );
}

/// A pass proves only the sign-in it was earned by, so a replacement starts without it.
#[test]
fn another_sign_in_never_inherits_a_pass() {
    let (_dir, cache, pass_file) = license_files();
    let lock = lock_checks(&cache).unwrap();
    save_pass(&pass_file, Some(&pass(T0))).unwrap();
    std::fs::write(&cache, b"{}").unwrap();
    record_check(&lock, &cache, &pass_file, Check::Verified, "a", T0)
        .1
        .unwrap();
    for check in [Check::Unverifiable, Check::NotOwned] {
        let (verdict, _) = record_check(&lock, &cache, &pass_file, check, "b", T0 + DAY);
        assert_eq!(verdict, Verdict::Deny(check));
    }
    assert_eq!(load_pass(&pass_file), None);
}

/// A failed helper names no sign-in; that must not read as a replacement.
#[test]
fn an_unknown_sign_in_keeps_the_pass() {
    let (_dir, cache, pass_file) = license_files();
    let lock = lock_checks(&cache).unwrap();
    std::fs::write(&cache, b"{}").unwrap();
    record_check(&lock, &cache, &pass_file, Check::Verified, "a", T0)
        .1
        .unwrap();
    let (verdict, saved) =
        record_check(&lock, &cache, &pass_file, Check::Unverifiable, "", T0 + DAY);
    saved.unwrap();
    assert_eq!(verdict, Verdict::Allow);
    assert_eq!(
        load_pass(&pass_file).map(|pass| pass.sign_in),
        Some("a".into())
    );
}

#[test]
fn removal_waits_for_no_check() {
    let (_dir, cache, pass_file) = license_files();
    let lock = lock_checks(&cache).unwrap();
    assert_eq!(
        forget_license(&cache, &pass_file).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(lock);
    forget_license(&cache, &pass_file).unwrap();
}

#[test]
fn client_ids_must_be_guids() {
    assert!(valid_client_id("00000000-0000-0000-0000-00000000abcd"));
    assert!(!valid_client_id("00000000000000000000000000000000abcd"));
    assert!(!valid_client_id("not-a-client-id"));
    assert!(!valid_client_id("00000000-0000-0000-0000-00000000abc "));
}

/// A partial cleanup must drop the pass yet keep the credentials, so removal can be retried.
#[test]
fn a_failed_forget_drops_the_pass_and_keeps_the_credentials() {
    let (dir, cache, pass_file) = license_files();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, b"{}").unwrap();
    save_pass(&pass_file, Some(&pass(T0))).unwrap();
    // A directory beside the credentials cannot be removed as a file.
    std::fs::create_dir_all(dir.path().join("license/microsoft-token.json.stuck/inner")).unwrap();
    assert!(forget_license(&cache, &pass_file).is_err());
    assert_eq!(load_pass(&pass_file), None);
    assert!(cache.exists());
}
