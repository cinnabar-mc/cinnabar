use super::*;
use crate::FogSuppression;

const NONE: FogSuppression = FogSuppression {
    air: false,
    water: false,
    lava: false,
};

fn source(init: &str, frame: &str, denied: bool) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("fog.wat")
        .replace("$ENVIRONMENT", &format!("{name}/environment@{version}"))
        .replace("$ERROR", if denied { "1" } else { "0" })
        .replace("$INIT", init)
        .replace("$FRAME", frame)
}

fn load(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fog.wat");
    std::fs::write(&path, source(init, frame, !granted)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            environment: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

#[test]
fn fog_suppression_is_denied_without_the_environment_grant() {
    let (_directory, mut host) = load("i32.const 7 call $set", "i32.const 7 call $set", false);
    assert_eq!(host.fog_suppression(), NONE);
    host.frame(false).unwrap();
    assert_eq!(host.fog_suppression(), NONE);
    assert!(host.is_active());
}

#[test]
fn each_medium_is_requested_independently() {
    let cases = [
        (1, (true, false, false)),
        (2, (false, true, false)),
        (4, (false, false, true)),
        (5, (true, false, true)),
        (7, (true, true, true)),
    ];
    for (mask, (air, water, lava)) in cases {
        let (_directory, mut host) = load("", &format!("i32.const {mask} call $set"), true);
        host.frame(false).unwrap();
        assert_eq!(
            host.fog_suppression(),
            FogSuppression { air, water, lava },
            "mask {mask}"
        );
    }
}

#[test]
fn fog_suppression_commits_after_success_and_is_retained() {
    let (_directory, mut host) = load("", "i32.const 3 call $set", true);
    assert_eq!(host.fog_suppression(), NONE);
    host.frame(false).unwrap();
    let expected = FogSuppression {
        air: true,
        water: true,
        lava: false,
    };
    assert_eq!(host.fog_suppression(), expected);
    let (_directory, mut host) = load("i32.const 3 call $set", "", true);
    assert_eq!(host.fog_suppression(), expected);
    host.frame(false).unwrap();
    assert_eq!(
        host.fog_suppression(),
        expected,
        "kept without republishing"
    );
}

#[test]
fn the_empty_set_restores_native_fog() {
    let (_directory, mut host) = load("i32.const 7 call $set", "i32.const 0 call $set", true);
    assert_ne!(host.fog_suppression(), NONE);
    host.frame(false).unwrap();
    assert_eq!(host.fog_suppression(), NONE);
}

#[test]
fn a_failed_callback_publishes_nothing_and_a_trap_revokes_it() {
    let (_directory, mut host) = load("", "i32.const 7 call $set unreachable", true);
    assert!(host.frame(false).is_err());
    assert_eq!(host.fog_suppression(), NONE);
    assert!(!host.is_active());
    let (_directory, mut host) = load(
        "i32.const 7 call $set",
        "i32.const 0 call $set unreachable",
        true,
    );
    assert_ne!(host.fog_suppression(), NONE);
    assert!(host.frame(false).is_err());
    assert_eq!(host.fog_suppression(), NONE);
}

#[test]
fn a_successful_reload_resets_it_and_a_rejected_one_keeps_it() {
    let (directory, mut host) = load("i32.const 7 call $set", "", true);
    let path = directory.path().join("fog.wat");
    std::fs::write(&path, source("unreachable", "", false)).unwrap();
    assert!(host.reload_if_changed().is_err());
    assert_ne!(host.fog_suppression(), NONE);
    std::fs::write(&path, source("", "", false)).unwrap();
    assert!(host.reload_if_changed().unwrap());
    assert_eq!(host.fog_suppression(), NONE);
}

#[test]
fn fog_shares_the_environment_callback_budget() {
    let frame = "i32.const 1 call $set ".repeat(8);
    let (_directory, mut host) = load("", &frame, true);
    host.frame(false).unwrap();
    assert!(host.fog_suppression().air);
    let frame = "i32.const 1 call $set ".repeat(9);
    let (_directory, mut host) = load("", &frame, true);
    assert!(host.frame(false).is_err(), "a ninth import traps");
    assert_eq!(host.fog_suppression(), NONE);
}

#[test]
fn loaded_mods_combine_what_they_suppress() {
    let air = FogSuppression { air: true, ..NONE };
    let lava = FogSuppression { lava: true, ..NONE };
    assert_eq!(
        air.union(lava),
        FogSuppression {
            air: true,
            water: false,
            lava: true
        }
    );
    assert_eq!(air.union(NONE), air);
    assert_eq!(NONE.union(NONE), NONE);
}
