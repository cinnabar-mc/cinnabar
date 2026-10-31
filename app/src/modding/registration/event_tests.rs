use super::recovery_tests::{select_entry, wait_update};
use super::*;

/// Completed work must reach the frame even when periodic file observation is far away.
#[test]
fn completed_admission_wakes_without_waiting_for_file_poll() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = candidate(directory.path(), "completion.wasm", "Hello");
    select_entry(directory.path(), prepared.registration.clone());
    let watcher =
        Watcher::start_with_poll_interval(directory.path().into(), Duration::from_secs(3600))
            .unwrap();
    let update = wait_update(
        &watcher,
        "completed admission wakes the coordinator",
        |update| !update.changes.is_empty(),
    );
    assert!(
        update.changes[0]
            .result
            .as_ref()
            .unwrap()
            .frame_host()
            .unwrap()
            .is_active()
    );
}

/// World authority changes use the normal observer and loader without waiting for file edits.
#[test]
fn authority_changes_wake_admission_without_a_registration_edit() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = candidate(directory.path(), "authority.wasm", "Hello");
    let entry = prepared.registration.clone();
    select_entry(directory.path(), entry.clone());
    let watcher =
        Watcher::start_with_poll_interval(directory.path().into(), Duration::from_secs(3600))
            .unwrap();
    let mut update = wait_update(&watcher, "initial authority fixture admission", |update| {
        !update.changes.is_empty()
    });
    let candidate = *update.changes.remove(0).result.unwrap();
    let identity = candidate.identity;
    watcher.send(Message::Acknowledge {
        generation: update.generation,
        request_id: "authority-ready".into(),
        installed: BTreeMap::from([(
            entry.component.clone(),
            Admitted {
                source: identity,
                drawing: None,
            },
        )]),
        statuses: Vec::new(),
    });
    test_time::eventually("initial authority admission acknowledged", || {
        fs::read(directory.path().join(STATUS_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StatusList>(&bytes).ok())
            .is_some_and(|status| status.request_id == "authority-ready")
    });
    let candidate::Runtime::Frame(prepared) = candidate.runtime else {
        panic!("frame fixture")
    };
    let mut runtime = ModRuntime::new(
        prepared.host,
        HostMetadata {
            font: prepared.font,
            branding: prepared.branding,
            owner: mod_ui::PanelUiOwner::Registration(1),
            registration: Some(RegisteredHost { entry, identity }),
        },
    );
    let session = mod_api::drawing::DrawingSession {
        connection: 1,
        ..Default::default()
    };
    runtime.hosts.host_mut(0).bind_drawing_session(session);
    watcher.remember_settings(Some(&runtime));
    let update = wait_update(&watcher, "authority change wakes admission", |update| {
        !update.changes.is_empty()
    });
    let host = update.changes[0]
        .result
        .as_ref()
        .unwrap()
        .frame_host()
        .unwrap();
    assert!(host.is_active());
    assert_eq!(host.drawing_session(), Some(session));
    assert!(!host.needs_drawing_restart());
}
