//! Exact, synthetic website/API handoff replay through the native public HTTP installer.

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{cell::RefCell, io::Read, net::TcpListener};

#[derive(Deserialize)]
struct Handoff {
    public_key: String,
    xuid: String,
    selected: CinnabarModEntry,
    exact_path: String,
    release_response: String,
    artifact: String,
    revocations: SignedRevocations,
    withdrawn: SignedRevocations,
    checked_at: u64,
}

/// Parses the synthetic website handoff with its throwaway signer and decoded artifact.
fn handoff() -> (Handoff, Keys, Vec<u8>) {
    let fixture: Handoff =
        serde_json::from_str(include_str!("fixtures/website-send.json")).unwrap();
    let keys = Keys::parse(
        &serde_json::to_vec(&serde_json::json!({"ed25519":[fixture.public_key]})).unwrap(),
    )
    .unwrap();
    let bytes = STANDARD.decode(&fixture.artifact).unwrap();
    (fixture, keys, bytes)
}

/// Replays original response bytes, checks exact paths, and refuses credential-bearing requests.
fn serve(
    responses: Vec<(String, Vec<u8>, Option<String>)>,
) -> (Transport, std::thread::JoinHandle<()>) {
    serve_status(
        responses
            .into_iter()
            .map(|(path, body, signature)| (path, 200, body, signature))
            .collect(),
    )
}

/// Serves exact bounded responses, including empty update lookups.
pub(super) fn serve_status(
    responses: Vec<(String, u16, Vec<u8>, Option<String>)>,
) -> (Transport, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for (path, status, body, signature) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let mut buffer = [0; 512];
                let size = stream.read(&mut buffer).unwrap();
                assert_ne!(size, 0);
                request.extend_from_slice(&buffer[..size]);
                assert!(request.len() <= 8192);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
            let headers = request.to_ascii_lowercase();
            assert!(!headers.contains("authorization:") && !headers.contains("cookie:"));
            let signature = signature
                .map(|value| format!("X-Cinnabar-Signature: {value}\r\n"))
                .unwrap_or_default();
            write!(
                stream,
                "HTTP/1.1 {status} Response\r\nConnection: close\r\n{signature}Content-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    (loopback(address), server)
}

/// Builds plain-HTTP transport to a loopback test origin.
fn loopback(address: std::net::SocketAddr) -> Transport {
    Transport {
        origin: format!("http://{address}").parse().unwrap(),
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    }
}

/// Binds an origin that never answers; `accepted` then proves whether any request was made.
fn idle() -> (Transport, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    (loopback(listener.local_addr().unwrap()), listener)
}

/// Reports whether a client connected to an idle origin.
fn accepted(listener: &TcpListener) -> bool {
    listener.set_nonblocking(true).unwrap();
    match listener.accept() {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) => panic!("{error}"),
    }
}

/// Builds a worker over the throwaway signer for one account epoch.
fn worker<'a>(
    runtime: &'a tokio::runtime::Runtime,
    transport: &'a Transport,
    mods: &'a Path,
    keys: &'a Keys,
    limit: usize,
    guard: &Arc<Guard>,
    outgoing: &'a Sender<Downloaded>,
) -> Worker<'a> {
    Worker {
        runtime,
        transport,
        mods,
        keys: Some(keys),
        limit,
        guard: guard.clone(),
        generation: Arc::new(AtomicU64::new(1)),
        outgoing,
        deliveries: Deliveries::default(),
        refresh_at: 0,
    }
}

/// Builds a current-thread runtime like the worker's own.
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Counts delivery attempts announced through status messages.
fn attempts(notes: &RefCell<Vec<String>>) -> usize {
    notes
        .borrow()
        .iter()
        .filter(|note| note.starts_with("Preparing"))
        .count()
}

#[test]
fn website_send_downloads_disabled_and_signed_withdrawal_holds_the_installed_component() {
    let (fixture, keys, bytes) = handoff();
    let shipped: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../data/marketplace-keys.json")).unwrap();
    assert!(
        !shipped["ed25519"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(fixture.public_key)),
        "throwaway test signer must never be a shipped trust root"
    );
    let (transport, server) = serve(vec![
        (
            "/v1/mods/revocations".into(),
            fixture.revocations.body.as_bytes().to_vec(),
            Some(fixture.revocations.signature.clone()),
        ),
        (
            fixture.exact_path,
            fixture.release_response.into_bytes(),
            None,
        ),
        (
            format!("/v1/mods/assets/{}", fixture.selected.sha256),
            bytes.clone(),
            None,
        ),
        (
            "/v1/mods/revocations".into(),
            fixture.withdrawn.body.as_bytes().to_vec(),
            Some(fixture.withdrawn.signature.clone()),
        ),
    ]);
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        refresh(&transport, root.path(), &keys, &guard, || {
            fixture.checked_at
        })
        .await
        .unwrap();
        let prepared = download(
            &transport,
            root.path(),
            &keys,
            &fixture.xuid,
            &fixture.selected,
            bytes.len(),
            (Arc::new(AtomicU64::new(1)), 1, guard.clone()),
            || fixture.checked_at,
        )
        .await
        .unwrap();
        assert_eq!(prepared.bytes().unwrap(), bytes);
        let installed = prepared.install(fixture.checked_at, |_| Ok(())).unwrap();
        assert!(
            !root.path().join("local-mod.json").exists(),
            "download cannot enable the component or approve permissions"
        );
        assert!(guard.allows(&installed, fixture.checked_at));
        refresh(&transport, root.path(), &keys, &guard, || {
            fixture.checked_at
        })
        .await
        .unwrap();
        assert!(!guard.allows(&installed, fixture.checked_at));
        assert!(
            guard
                .reason(&installed, fixture.checked_at)
                .contains("Synthetic integration withdrawal")
        );
    });
    server.join().unwrap();
}

#[test]
fn account_switch_retries_cancelled_delivery_but_keeps_durable_consumed_sends() {
    let (fixture, keys, bytes) = handoff();
    let mut responses = vec![(
        "/v1/mods/revocations".into(),
        fixture.revocations.body.as_bytes().to_vec(),
        Some(fixture.revocations.signature.clone()),
    )];
    for _ in 0..2 {
        responses.push((
            fixture.exact_path.clone(),
            fixture.release_response.as_bytes().to_vec(),
            None,
        ));
        responses.push((
            format!("/v1/mods/assets/{}", fixture.selected.sha256),
            bytes.clone(),
            None,
        ));
    }
    let (transport, server) = serve(responses);
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    let generation = Arc::new(AtomicU64::new(1));
    let (outgoing, incoming) = crossbeam_channel::bounded(1);
    let key = request(&fixture.xuid, &fixture.selected);
    let mut deliveries = Deliveries::default();
    // Starts a new account epoch whose identity reply arrives in time.
    let switch = |epoch| {
        generation.store(epoch, Ordering::SeqCst);
        guard.configure(epoch);
        guard.account(epoch, fixture.xuid.clone());
    };
    // Re-derives queue admission for an epoch from durable receipts.
    let refresh_queue = |deliveries: &mut Deliveries, epoch| {
        deliveries.refresh(
            epoch,
            root.path(),
            Some(&keys),
            bytes.len(),
            &guard,
            fixture.checked_at,
        )
    };
    switch(1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        refresh(&transport, root.path(), &keys, &guard, || {
            fixture.checked_at
        })
        .await
        .unwrap();
        for epoch in [1, 3] {
            switch(epoch);
            refresh_queue(&mut deliveries, epoch);
            assert!(!deliveries.requests.contains(&key));
            let prepared = download(
                &transport,
                root.path(),
                &keys,
                &fixture.xuid,
                &fixture.selected,
                bytes.len(),
                (generation.clone(), epoch, guard.clone()),
                || fixture.checked_at,
            )
            .await
            .unwrap();
            deliveries.queue(key.clone(), prepared, &outgoing, fixture.checked_at);
            refresh_queue(&mut deliveries, epoch);
            assert!(
                deliveries.requests.contains(&key),
                "pending delivery must remain deduplicated in the same epoch"
            );
            let prepared = incoming.recv().unwrap();
            if epoch == 1 {
                generation.store(2, Ordering::SeqCst);
                guard.configure(2);
                assert!(prepared.install(fixture.checked_at, |_| Ok(())).is_err());
                let name = storage::filename(&fixture.xuid, &fixture.selected).unwrap();
                assert!(!root.path().join(name).exists());
            } else {
                let installed = prepared.install(fixture.checked_at, |_| Ok(())).unwrap();
                switch(4);
                refresh_queue(&mut deliveries, 4);
                assert!(
                    deliveries.requests.contains(&key),
                    "successful install must stay consumed after switching accounts"
                );
                assert!(guard.allows(&installed, fixture.checked_at));
                fs::remove_file(installed).unwrap();
                switch(5);
                refresh_queue(&mut deliveries, 5);
                assert!(
                    deliveries.requests.contains(&key),
                    "local removal must stay consumed after switching accounts"
                );
                assert!(storage::removed(
                    root.path(),
                    &fixture.xuid,
                    &fixture.selected,
                    &keys,
                    bytes.len(),
                    fixture.checked_at
                ));
            }
        }
    });
    server.join().unwrap();
}

#[test]
fn held_selection_is_skipped_without_network_io() {
    let (fixture, keys, bytes) = handoff();
    let (transport, listener) = idle();
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    let runtime = runtime();
    let (outgoing, _incoming) = crossbeam_channel::bounded(1);
    let mut worker = worker(
        &runtime,
        &transport,
        root.path(),
        &keys,
        bytes.len(),
        &guard,
        &outgoing,
    );
    worker.refresh_at = u64::MAX;
    let library = CinnabarModLibrary {
        xuid: fixture.xuid.clone(),
        mods: vec![fixture.selected.clone()],
    };
    let notes = RefCell::new(Vec::new());
    let notify = |note| notes.borrow_mut().push(note);
    // No signed revocations are current, so freshness has lapsed.
    worker.pass(
        1,
        &library,
        |_| true,
        |_| {},
        &|| fixture.checked_at,
        &notify,
    );
    // Fresh evidence still holds a withdrawn hash.
    guard.revocations(
        fixture
            .revocations
            .verify(&keys, fixture.checked_at, 0)
            .unwrap(),
    );
    guard.hold(&fixture.selected.sha256);
    worker.pass(
        1,
        &library,
        |_| true,
        |_| {},
        &|| fixture.checked_at,
        &notify,
    );
    assert!(!accepted(&listener), "held selections must not be fetched");
    assert_eq!(attempts(&notes), 0);
    assert!(worker.deliveries.failures.is_empty());
}

#[test]
fn failed_send_backs_off_exponentially_and_resets_on_account_change() {
    let (fixture, keys, bytes) = handoff();
    let garbage = (fixture.exact_path.clone(), b"not json".to_vec(), None);
    let (transport, server) = serve(vec![garbage.clone(), garbage]);
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    guard.revocations(
        fixture
            .revocations
            .verify(&keys, fixture.checked_at, 0)
            .unwrap(),
    );
    let runtime = runtime();
    let (outgoing, _incoming) = crossbeam_channel::bounded(1);
    let mut worker = worker(
        &runtime,
        &transport,
        root.path(),
        &keys,
        bytes.len(),
        &guard,
        &outgoing,
    );
    worker.refresh_at = u64::MAX;
    let start = fixture.checked_at;
    worker
        .deliveries
        .refresh(1, root.path(), Some(&keys), bytes.len(), &guard, start);
    let library = CinnabarModLibrary {
        xuid: fixture.xuid.clone(),
        mods: vec![fixture.selected.clone()],
    };
    let key = request(&fixture.xuid, &fixture.selected);
    let notes = RefCell::new(Vec::new());
    let notify = |note| notes.borrow_mut().push(note);
    let clock = AtomicU64::new(start);
    let now = || clock.load(Ordering::SeqCst);
    let pass_at = |worker: &mut Worker, at| {
        clock.store(at, Ordering::SeqCst);
        worker.pass(1, &library, |_| true, |_| {}, &now, &notify);
    };
    pass_at(&mut worker, start);
    assert_eq!(attempts(&notes), 1);
    assert_eq!(
        worker.deliveries.failures.get(&key),
        Some(&Failure {
            count: 1,
            retry_at: start + 30_000
        })
    );
    pass_at(&mut worker, start + 5_000);
    pass_at(&mut worker, start + 29_999);
    assert_eq!(attempts(&notes), 1, "a failed send waits for its backoff");
    pass_at(&mut worker, start + 30_000);
    assert_eq!(attempts(&notes), 2);
    assert_eq!(
        worker.deliveries.failures.get(&key),
        Some(&Failure {
            count: 2,
            retry_at: start + 30_000 + 60_000
        })
    );
    server.join().unwrap();
    let delays: Vec<u64> = (1..=8).map(retry_delay).collect();
    assert_eq!(
        delays,
        [30, 60, 120, 240, 480, 960, 1800, 1800].map(|seconds| seconds * 1000)
    );
    assert_eq!(retry_delay(u32::MAX), 30 * 60_000);
    worker
        .deliveries
        .refresh(2, root.path(), Some(&keys), bytes.len(), &guard, now());
    assert!(worker.deliveries.failures.is_empty());
    assert!(
        worker.deliveries.due(&key, now()),
        "an account change retries immediately"
    );
}

#[test]
fn removal_requests_wait_for_the_failure_backoff() {
    let (fixture, keys, bytes) = handoff();
    let (transport, listener) = idle();
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    guard.revocations(
        fixture
            .revocations
            .verify(&keys, fixture.checked_at, 0)
            .unwrap(),
    );
    let response: serde_json::Value = serde_json::from_str(&fixture.release_response).unwrap();
    let release: SignedRelease = serde_json::from_value(response["release"].clone()).unwrap();
    let mut file = tempfile::NamedTempFile::new_in(root.path()).unwrap();
    file.write_all(&bytes).unwrap();
    let installed = Downloaded {
        file,
        manifest: release
            .verify(&keys, &fixture.selected, bytes.len(), fixture.checked_at)
            .unwrap(),
        receipt: Receipt {
            owner: fixture.xuid.clone(),
            selected: fixture.selected.clone(),
            release,
            consumed: Vec::new(),
            migration: None,
        },
        generation: Arc::new(AtomicU64::new(1)),
        epoch: 1,
        guard: guard.clone(),
        mods: root.path().into(),
        previous: None,
        authorization: None,
    }
    .install(fixture.checked_at, |_| Ok(()))
    .unwrap();
    fs::remove_file(installed).unwrap();
    let runtime = runtime();
    let (outgoing, _incoming) = crossbeam_channel::bounded(1);
    let mut worker = worker(
        &runtime,
        &transport,
        root.path(),
        &keys,
        bytes.len(),
        &guard,
        &outgoing,
    );
    worker.refresh_at = u64::MAX;
    let start = fixture.checked_at;
    worker
        .deliveries
        .refresh(1, root.path(), Some(&keys), bytes.len(), &guard, start);
    let library = CinnabarModLibrary {
        xuid: fixture.xuid.clone(),
        mods: vec![fixture.selected.clone()],
    };
    let removals = std::cell::Cell::new(0);
    let notify = |_| {};
    let clock = AtomicU64::new(start);
    let now = || clock.load(Ordering::SeqCst);
    let pass_at = |worker: &mut Worker, at| {
        clock.store(at, Ordering::SeqCst);
        worker.pass(
            1,
            &library,
            |_| true,
            |_| removals.set(removals.get() + 1),
            &now,
            &notify,
        );
        removals.get()
    };
    assert_eq!(pass_at(&mut worker, start), 1);
    assert_eq!(pass_at(&mut worker, start + 5_000), 1);
    assert_eq!(pass_at(&mut worker, start + 29_999), 1);
    assert_eq!(pass_at(&mut worker, start + 30_000), 2);
    assert_eq!(pass_at(&mut worker, start + 30_000 + 59_999), 2);
    assert_eq!(pass_at(&mut worker, start + 30_000 + 60_000), 3);
    assert!(!accepted(&listener), "removals never fetch the release");
}

#[test]
fn long_pass_refreshes_revocations_once_their_refresh_time_passes() {
    let (fixture, keys, bytes) = handoff();
    let revocations = (
        "/v1/mods/revocations".to_string(),
        fixture.revocations.body.as_bytes().to_vec(),
        Some(fixture.revocations.signature.clone()),
    );
    let mut responses = vec![revocations.clone()];
    for index in 0..2 {
        if index > 0 {
            responses.push(revocations.clone());
        }
        responses.push((
            fixture.exact_path.clone(),
            fixture.release_response.as_bytes().to_vec(),
            None,
        ));
        responses.push((
            format!("/v1/mods/assets/{}", fixture.selected.sha256),
            bytes.clone(),
            None,
        ));
    }
    let (transport, server) = serve(responses);
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    let runtime = runtime();
    let (outgoing, incoming) = crossbeam_channel::bounded(2);
    let mut worker = worker(
        &runtime,
        &transport,
        root.path(),
        &keys,
        bytes.len(),
        &guard,
        &outgoing,
    );
    let mut resent = fixture.selected.clone();
    resent.sent_at = "2026-10-24T14:44:20.420Z".into();
    let library = CinnabarModLibrary {
        xuid: fixture.xuid.clone(),
        mods: vec![fixture.selected.clone(), resent],
    };
    let notes = RefCell::new(Vec::new());
    let notify = |note| notes.borrow_mut().push(note);
    let clock = AtomicU64::new(fixture.checked_at);
    let now = || clock.load(Ordering::SeqCst);
    worker.refresh_due(Some(&library), &now, &notify);
    // Each delivery takes a full refresh interval, so the second one needs a new refresh.
    let slow = |_: &CinnabarModEntry| {
        clock.fetch_add(REFRESH_INTERVAL_MS, Ordering::SeqCst);
        true
    };
    worker.pass(1, &library, slow, |_| {}, &now, &notify);
    assert_eq!(incoming.try_iter().count(), 2, "{:?}", notes.borrow());
    assert_eq!(
        worker.refresh_at,
        fixture.checked_at + 2 * REFRESH_INTERVAL_MS
    );
    server.join().unwrap();
}

/// Serves one signed revocation snapshot per expected refresh.
fn serve_revocations(fixture: &Handoff, count: usize) -> (Transport, std::thread::JoinHandle<()>) {
    serve(
        (0..count)
            .map(|_| {
                (
                    "/v1/mods/revocations".into(),
                    fixture.revocations.body.as_bytes().to_vec(),
                    Some(fixture.revocations.signature.clone()),
                )
            })
            .collect(),
    )
}

#[test]
fn revocations_refresh_only_while_an_installed_receipt_or_a_selection_needs_them() {
    let (fixture, keys, bytes) = handoff();
    let (transport, server) = serve_revocations(&fixture, 2);
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    let runtime = runtime();
    let (outgoing, _incoming) = crossbeam_channel::bounded(1);
    let mut worker = worker(
        &runtime,
        &transport,
        root.path(),
        &keys,
        bytes.len(),
        &guard,
        &outgoing,
    );
    let notes = RefCell::new(Vec::new());
    let notify = |note| notes.borrow_mut().push(note);
    let clock = AtomicU64::new(fixture.checked_at);
    let now = || clock.load(Ordering::SeqCst);
    let mut library = CinnabarModLibrary {
        xuid: fixture.xuid.clone(),
        mods: Vec::new(),
    };
    worker.refresh_due(None, &now, &notify);
    worker.refresh_due(Some(&library), &now, &notify);
    assert_eq!(worker.refresh_at, 0, "nothing to protect makes no request");
    assert_eq!(guard.checkpoint(), 0);
    assert!(notes.borrow().is_empty());

    library.mods.push(fixture.selected.clone());
    worker.refresh_due(Some(&library), &now, &notify);
    assert_eq!(
        guard.checkpoint(),
        fixture
            .revocations
            .verify(&keys, now(), 0)
            .unwrap()
            .issued_at
    );
    assert_eq!(worker.refresh_at, fixture.checked_at + REFRESH_INTERVAL_MS);

    // Without selections, an installed managed file keeps its evidence fresh.
    let response: serde_json::Value = serde_json::from_str(&fixture.release_response).unwrap();
    let release: SignedRelease = serde_json::from_value(response["release"].clone()).unwrap();
    let path = root
        .path()
        .join(storage::filename(&fixture.xuid, &fixture.selected).unwrap());
    fs::write(&path, &bytes).unwrap();
    guard.installed(
        path,
        fixture.xuid.clone(),
        release
            .verify(&keys, &fixture.selected, bytes.len(), now())
            .unwrap(),
    );
    clock.fetch_add(REFRESH_INTERVAL_MS, Ordering::SeqCst);
    worker.refresh_due(None, &now, &notify);
    assert_eq!(worker.refresh_at, now() + REFRESH_INTERVAL_MS);
    assert!(notes.borrow().is_empty(), "{:?}", notes.borrow());
    server.join().unwrap();
}

#[test]
fn refresh_failures_are_reported_only_when_a_managed_mod_or_pending_selection_is_affected() {
    let (fixture, keys, bytes) = handoff();
    // An unsigned snapshot fails verification on every refresh.
    let unsigned = ("/v1/mods/revocations".to_string(), b"{}".to_vec(), None);
    let (transport, server) = serve(vec![unsigned.clone(), unsigned]);
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    guard.configure(1);
    guard.account(1, fixture.xuid.clone());
    let runtime = runtime();
    let (outgoing, _incoming) = crossbeam_channel::bounded(1);
    let mut worker = worker(
        &runtime,
        &transport,
        root.path(),
        &keys,
        bytes.len(),
        &guard,
        &outgoing,
    );
    let notes = RefCell::new(Vec::new());
    let notify = |note| notes.borrow_mut().push(note);
    let clock = AtomicU64::new(fixture.checked_at);
    let now = || clock.load(Ordering::SeqCst);
    let mut withdrawn = fixture.selected.clone();
    withdrawn.pulled = true;
    let library = CinnabarModLibrary {
        xuid: fixture.xuid.clone(),
        mods: vec![withdrawn],
    };
    worker.refresh_due(Some(&library), &now, &notify);
    assert_eq!(worker.refresh_at, fixture.checked_at + REFRESH_INTERVAL_MS);
    assert!(
        notes.borrow().is_empty(),
        "an unaffected library must keep its Mods message: {:?}",
        notes.borrow()
    );

    clock.fetch_add(REFRESH_INTERVAL_MS, Ordering::SeqCst);
    let library = CinnabarModLibrary {
        xuid: fixture.xuid.clone(),
        mods: vec![fixture.selected.clone()],
    };
    worker.refresh_due(Some(&library), &now, &notify);
    assert_eq!(notes.borrow().len(), 1);
    assert!(notes.borrow()[0].starts_with("Marketplace checks could not refresh"));
    assert!(!guard.permits(&fixture.xuid, &fixture.selected.sha256, now()));
    server.join().unwrap();
}

#[test]
fn asset_timeout_lets_a_max_size_component_finish_on_a_slow_link() {
    // Comfortably above mod_host::MAX_COMPONENT_SOURCE_BYTES (about 20 MiB), which
    // launcher-host cannot import.
    let bytes: u64 = 32 * 1024 * 1024;
    // A 1.25 Mbit/s link moves 156,250 bytes per second.
    let transfer = Duration::from_secs(bytes.div_ceil(156_250));
    assert!(asset_timeout(bytes) >= transfer);
    assert!(asset_timeout(bytes) <= MAX_ASSET_TIMEOUT);
    assert_eq!(asset_timeout(1), REQUEST_TIMEOUT);
    assert_eq!(asset_timeout(u64::MAX), MAX_ASSET_TIMEOUT);
}
