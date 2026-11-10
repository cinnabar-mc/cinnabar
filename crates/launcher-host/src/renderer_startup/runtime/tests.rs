use super::*;

/// Models a selected renderer without requiring a native GPU.
fn attempt(backend: Backend, explicit: bool) -> Attempt {
    Attempt {
        backend,
        adapter: None,
        stage: Stage::FirstPresentation,
        explicit,
    }
}

#[test]
fn device_loss_survives_orderly_exit_before_or_after_presentation() {
    for presented in [false, true] {
        for explicit in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut guard = StartupGuard::open(root.path()).unwrap();
            guard.begin(attempt(Backend::Dx12, explicit)).unwrap();
            if presented {
                guard.presented().unwrap();
            }
            guard.device_lost().unwrap();
            guard.cancel().unwrap();
            assert!(StartupGuard::pending(root.path()).unwrap().is_none());
            let next = StartupGuard::open(root.path()).unwrap();
            assert!(next.recovery.working.is_none());
            if explicit {
                assert_eq!(next.recovery.crashed_choice(), Some(Backend::Dx12));
                assert!(next.recovery.failed_backends().is_empty());
            } else {
                assert_eq!(next.recovery.failed_backends(), [Backend::Dx12]);
                assert_eq!(
                    launcher::graphics_backend::candidates(
                        launcher::graphics_backend::Platform::Windows.backends(),
                        launcher::graphics_backend::Platform::Windows.backends(),
                        None,
                        None,
                        next.recovery.failed_backends(),
                    ),
                    [Backend::Vulkan]
                );
            }
        }
    }
}

#[test]
fn runtime_loss_merges_failures_recorded_by_a_later_client() {
    let root = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(root.path()).unwrap();
    first.begin(attempt(Backend::Dx12, false)).unwrap();
    first.presented().unwrap();
    let mut second = StartupGuard::open(root.path()).unwrap();
    second.begin(attempt(Backend::Vulkan, false)).unwrap();
    drop(second);
    let mut recovered = StartupGuard::open(root.path()).unwrap();
    recovered.cancel().unwrap();
    first.device_lost().unwrap();
    let next = StartupGuard::open(root.path()).unwrap();
    assert_eq!(
        next.recovery.failed_backends(),
        [Backend::Vulkan, Backend::Dx12]
    );
}

#[test]
fn runtime_loss_cannot_modify_another_clients_pending_startup() {
    let root = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(root.path()).unwrap();
    first.begin(attempt(Backend::Dx12, false)).unwrap();
    first.presented().unwrap();
    let mut second = StartupGuard::open(root.path()).unwrap();
    second.begin(attempt(Backend::Vulkan, false)).unwrap();
    let sentinel = fs::read(root.path().join(SENTINEL_FILE)).unwrap();
    let choice = fs::read(root.path().join(CHOICE_FILE)).unwrap();
    assert!(first.device_lost().is_err());
    assert_eq!(fs::read(root.path().join(SENTINEL_FILE)).unwrap(), sentinel);
    assert_eq!(fs::read(root.path().join(CHOICE_FILE)).unwrap(), choice);
    second.cancel().unwrap();
    first.device_lost().unwrap();
    let next = StartupGuard::open(root.path()).unwrap();
    assert_eq!(next.recovery.failed_backends(), [Backend::Dx12]);
}

#[test]
fn runtime_loss_does_not_override_newer_versions_or_settings() {
    for changed_version in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut first = StartupGuard::open_version(root.path(), "one").unwrap();
        first.begin(attempt(Backend::Dx12, false)).unwrap();
        first.presented().unwrap();
        let version = if changed_version { "two" } else { "one" };
        let mut second = StartupGuard::open_version(root.path(), version).unwrap();
        if !changed_version {
            second.refresh_settings(1, false).unwrap();
        }
        second.cancel().unwrap();
        let choice = fs::read(root.path().join(CHOICE_FILE)).unwrap();
        first.device_lost().unwrap();
        assert_eq!(fs::read(root.path().join(CHOICE_FILE)).unwrap(), choice);
    }
}

#[test]
fn repeated_loss_cannot_invalidate_a_subsequent_successful_retry() {
    let root = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(root.path()).unwrap();
    first.begin(attempt(Backend::Dx12, false)).unwrap();
    first.presented().unwrap();
    first.device_lost().unwrap();
    let mut second = StartupGuard::open(root.path()).unwrap();
    second.begin(attempt(Backend::Dx12, true)).unwrap();
    second.presented().unwrap();
    first.device_lost().unwrap();
    let next = StartupGuard::open(root.path()).unwrap();
    assert!(next.recovery.failed_backends().is_empty());
    assert_eq!(
        next.recovery.working.as_ref().unwrap().backend,
        Backend::Dx12
    );
}

#[test]
fn a_cancelled_startup_cannot_blame_an_earlier_working_renderer() {
    let root = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(root.path()).unwrap();
    first.begin(attempt(Backend::Dx12, false)).unwrap();
    first.presented().unwrap();
    let mut cancelled = StartupGuard::open(root.path()).unwrap();
    cancelled.begin(attempt(Backend::Vulkan, false)).unwrap();
    cancelled.cancel().unwrap();
    cancelled.device_lost().unwrap();
    let next = StartupGuard::open(root.path()).unwrap();
    assert!(next.recovery.failed_backends().is_empty());
    assert_eq!(
        next.recovery.working.as_ref().unwrap().backend,
        Backend::Dx12
    );
}

#[test]
fn device_loss_after_controlled_shutdown_does_not_change_recovery() {
    let root = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(root.path()).unwrap();
    guard.begin(attempt(Backend::Dx12, false)).unwrap();
    guard.presented().unwrap();
    guard.cancel().unwrap();
    guard.device_lost().unwrap();
    let next = StartupGuard::open(root.path()).unwrap();
    assert!(next.recovery.failed_backends().is_empty());
    assert_eq!(
        next.recovery.working.as_ref().unwrap().backend,
        Backend::Dx12
    );
}
