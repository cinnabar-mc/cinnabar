use super::*;

/// A guest that reads its remaining fuel around a loop of `spin` iterations and traps unless
/// the reads fit `allowance` and the loop spent fuel.
fn load(allowance: u64) -> (tempfile::TempDir, anyhow::Result<ModHost>) {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let interface = format!("{name}/budget@{version}");
    assert_eq!(interface, crate::runtime::budget_host::INTERFACE);
    let source = include_str!("budget.wat")
        .replace("$BUDGET", &interface)
        .replace("$ALLOWANCE", &allowance.to_string());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("budget.wat");
    std::fs::write(&path, source).unwrap();
    let host = ModHost::load_with_grants(&path, ModGrants::default());
    (directory, host)
}

#[test]
fn callbacks_read_their_remaining_fuel_without_a_grant() {
    let (_directory, host) = load(FRAME_FUEL);
    let mut host = host.unwrap();
    host.frame(false).unwrap();
    assert!(
        host.is_active(),
        "the reads fell within the frame allowance"
    );
    let (_directory, host) = load(FRAME_FUEL / 2);
    let error = host.err().expect("init reads more than half the allowance");
    assert!(format!("{error:#}").contains("unreachable"), "{error:#}");
}
