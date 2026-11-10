//! Loopback HTTP regressions for account-owned, compatibility-bound marketplace updates.

use super::*;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};

const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
static APIS: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| serde_json::to_string(&[REVISION]).unwrap());

struct Fixture {
    root: tempfile::TempDir,
    signer: Ed25519KeyPair,
    keys: Keys,
    guard: Arc<Guard>,
    receipt: Receipt,
    path: PathBuf,
}

impl Fixture {
    /// Publishes an initial signed receipt for a live account without executing guest code.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let signer = Ed25519KeyPair::from_seed_unchecked(&[43; 32]).unwrap();
        let keys = Keys::parse(
            serde_json::json!({"ed25519":[STANDARD.encode(signer.public_key())]})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        let selected = CinnabarModEntry {
            slug: "test-mod".into(),
            version: "1.0.0".into(),
            sha256: format!("{:x}", Sha256::digest(b"old")),
            bytes: 3,
            sent_at: "fixture-send".into(),
            pulled: false,
        };
        let statement = serde_json::json!({"slug":selected.slug,"version":selected.version,"sha256":selected.sha256,"bytes":selected.bytes,"grants":{"hud":true},"approved_at":1000,"client_revision":REVISION,"api_revision":REVISION,"policy":"fixture","publication":"automatic"}).to_string();
        let release = SignedRelease {
            signature: URL_SAFE_NO_PAD.encode(signer.sign(statement.as_bytes())),
            statement,
        };
        let receipt = Receipt {
            owner: "11".into(),
            selected,
            release,
            consumed: Vec::new(),
            migration: None,
        };
        let guard = Arc::new(Guard::default());
        guard.configure(1);
        guard.account(1, "11".into());
        guard.revocations(super::super::super::verify::RevocationState {
            issued_at: 1000,
            expires_at: 100_000_000,
            revoked: HashMap::new(),
        });
        let path = root
            .path()
            .join(storage::filename(&receipt.owner, &receipt.selected).unwrap());
        let mut file = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        file.write_all(b"old").unwrap();
        Downloaded {
            file,
            manifest: receipt
                .release
                .verify(&keys, &receipt.selected, 1000, 3000)
                .unwrap(),
            receipt: receipt.clone(),
            generation: Arc::new(AtomicU64::new(1)),
            epoch: 1,
            guard: guard.clone(),
            mods: root.path().into(),
            previous: None,
            authorization: None,
        }
        .install(3000, |_| Ok(()))
        .unwrap();
        Self {
            root,
            signer,
            keys,
            guard,
            receipt,
            path,
        }
    }

    /// Signs an update after the caller has changed any evidence under test.
    fn release(&self, change: impl FnOnce(&mut serde_json::Value)) -> SignedRelease {
        let mut value: serde_json::Value =
            serde_json::from_str(&self.receipt.release.statement).unwrap();
        value["version"] = serde_json::json!("1.1.0");
        value["sha256"] = serde_json::json!(format!("{:x}", Sha256::digest(b"new")));
        change(&mut value);
        let statement = value.to_string();
        SignedRelease {
            signature: URL_SAFE_NO_PAD.encode(self.signer.sign(statement.as_bytes())),
            statement,
        }
    }

    /// Formats the complete expected query, asserting both compatibility parameters and installed identity.
    fn query(&self) -> String {
        let mut url = url::Url::parse("http://localhost/v1/mods/test-mod/update").unwrap();
        url.query_pairs_mut().extend_pairs([
            ("api_revisions", REVISION),
            ("host_revision", REVISION),
            ("current_version", "1.0.0"),
            ("current_sha256", self.receipt.selected.sha256.as_str()),
        ]);
        format!("{}?{}", url.path(), url.query().unwrap())
    }

    /// Runs one production update pass against exact loopback responses.
    fn pass(
        &self,
        release: SignedRelease,
        asset: Option<Vec<u8>>,
        revoked: bool,
    ) -> Vec<Downloaded> {
        let mut responses = vec![(
            self.query(),
            200,
            serde_json::json!({"release":release})
                .to_string()
                .into_bytes(),
            None,
        )];
        if let Some(asset) = asset {
            let value: serde_json::Value = serde_json::from_str(&release.statement).unwrap();
            responses.push((
                format!("/v1/mods/assets/{}", value["sha256"].as_str().unwrap()),
                200,
                asset,
                None,
            ));
        }
        let (transport, server) = super::super::tests::serve_status(responses);
        if revoked {
            let manifest: super::super::super::Manifest =
                serde_json::from_str(&release.statement).unwrap();
            self.guard.hold(&manifest.sha256);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (outgoing, incoming) = crossbeam_channel::bounded(2);
        let worker = Worker {
            runtime: &runtime,
            transport: &transport,
            mods: self.root.path(),
            keys: Some(&self.keys),
            limit: 1000,
            guard: self.guard.clone(),
            generation: Arc::new(AtomicU64::new(1)),
            outgoing: &outgoing,
            deliveries: Deliveries::default(),
            refresh_at: u64::MAX,
        };
        let policy = Policy::default();
        policy.configure(true, true, []);
        Updates::default().pass(
            &worker,
            1,
            &CinnabarModLibrary {
                xuid: "11".into(),
                mods: vec![self.receipt.selected.clone()],
            },
            &Host {
                revision: REVISION,
                api_revisions_json: &APIS,
            },
            &policy,
            || true,
            &|| 3000,
            &|_| {},
        );
        server.join().unwrap();
        incoming.try_iter().collect()
    }
}

#[test]
fn compatible_newer_updates_are_staged_with_the_same_owner_and_consumed_send() {
    let f = Fixture::new();
    let mut prepared = f.pass(f.release(|_| {}), Some(b"new".to_vec()), false);
    assert_eq!(prepared.len(), 1);
    let prepared = prepared.remove(0);
    assert_eq!(prepared.previous(), Some(f.path.as_path()));
    assert_eq!(prepared.receipt.owner, "11");
    assert_eq!(prepared.receipt.consumed, vec![f.receipt.selected.clone()]);
    let target = prepared.install(3000, |_| Ok(())).unwrap();
    assert_eq!(fs::read(target).unwrap(), b"new");
    assert_eq!(fs::read(&f.path).unwrap(), b"old");
}

#[test]
fn account_changes_cancel_prepared_updates_before_publication() {
    let f = Fixture::new();
    let mut prepared = f.pass(f.release(|_| {}), Some(b"new".to_vec()), false);
    assert_eq!(prepared.len(), 1);
    f.guard.configure(2);
    f.guard.account(2, "22".into());
    assert!(prepared.remove(0).install(3001, |_| Ok(())).is_err());
    assert_eq!(fs::read(&f.path).unwrap(), b"old");
    assert_eq!(crate::mod_library::files::installed(f.root.path()).len(), 1);
}

#[test]
fn periodic_checks_only_query_receipts_owned_by_the_live_account() {
    let f = Fixture::new();
    let (transport, server) = super::super::tests::serve_status(vec![
        (f.query(), 204, Vec::new(), None),
        (f.query(), 204, Vec::new(), None),
    ]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (outgoing, incoming) = crossbeam_channel::bounded(2);
    let worker = Worker {
        runtime: &runtime,
        transport: &transport,
        mods: f.root.path(),
        keys: Some(&f.keys),
        limit: 1000,
        guard: f.guard.clone(),
        generation: Arc::new(AtomicU64::new(1)),
        outgoing: &outgoing,
        deliveries: Deliveries::default(),
        refresh_at: u64::MAX,
    };
    let policy = Policy::default();
    policy.configure(true, true, []);
    let mut updates = Updates::default();
    let mut library = CinnabarModLibrary {
        xuid: "22".into(),
        mods: Vec::new(),
    };
    let host = Host {
        revision: REVISION,
        api_revisions_json: &APIS,
    };
    updates.pass(
        &worker,
        1,
        &library,
        &host,
        &policy,
        || true,
        &|| 3000,
        &|_| {},
    );
    assert!(updates.attempts.is_empty());
    library.xuid = "11".into();
    for now in [3000, 3001, 3000 + REFRESH_INTERVAL_MS] {
        updates.pass(
            &worker,
            1,
            &library,
            &host,
            &policy,
            || true,
            &|| now,
            &|_| {},
        );
        assert_eq!(updates.attempts[&f.path].count, 0);
        assert!(updates.attempts[&f.path].retry_at > now);
    }
    assert!(incoming.try_recv().is_err());
    assert_eq!(fs::read(&f.path).unwrap(), b"old");
    server.join().unwrap();
}

#[test]
fn invalid_update_evidence_never_changes_the_installed_release() {
    for failure in [
        "missing-api",
        "malformed-api",
        "incompatible-api",
        "slug",
        "equal",
        "older",
        "signature",
        "sha",
        "size",
        "revoked",
        "held",
    ] {
        let f = Fixture::new();
        let mut release = f.release(|value| match failure {
            "missing-api" => {
                value.as_object_mut().unwrap().remove("api_revision");
            }
            "incompatible-api" => value["api_revision"] = serde_json::json!("b".repeat(40)),
            "malformed-api" => value["api_revision"] = serde_json::json!("a".repeat(39)),
            "slug" => value["slug"] = serde_json::json!("another-mod"),
            "equal" => value["version"] = serde_json::json!("1.0.0"),
            "older" => value["version"] = serde_json::json!("0.9.0"),
            "sha" => value["sha256"] = serde_json::json!("0".repeat(64)),
            "size" => value["bytes"] = serde_json::json!(4),
            _ => {}
        });
        if failure == "signature" {
            release.statement.push(' ');
        }
        if failure == "revoked" {
            let manifest: super::super::super::Manifest =
                serde_json::from_str(&release.statement).unwrap();
            f.guard
                .revocations(super::super::super::verify::RevocationState {
                    issued_at: 2000,
                    expires_at: 100_000_000,
                    revoked: HashMap::from([(manifest.sha256, "withdrawn".into())]),
                });
        }
        let asset = matches!(failure, "sha" | "size").then(|| b"new".to_vec());
        assert!(
            f.pass(release, asset, failure == "held").is_empty(),
            "{failure}"
        );
        assert_eq!(fs::read(&f.path).unwrap(), b"old", "{failure}");
        assert_eq!(
            crate::mod_library::files::installed(f.root.path()).len(),
            1,
            "{failure}"
        );
    }
}

#[test]
fn no_content_and_hidden_listing_leave_receipts_and_files_unchanged() {
    for status in [204, 404] {
        let f = Fixture::new();
        let (transport, server) =
            super::super::tests::serve_status(vec![(f.query(), status, Vec::new(), None)]);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(
            runtime
                .block_on(lookup(
                    &transport,
                    &f.keys,
                    &f.receipt,
                    &Host {
                        revision: REVISION,
                        api_revisions_json: &APIS
                    },
                    &[REVISION.into()],
                    1000,
                    3000
                ))
                .unwrap()
                .is_none()
        );
        server.join().unwrap();
        assert_eq!(fs::read(&f.path).unwrap(), b"old");
    }
}

#[test]
fn update_preferences_offer_manual_install_and_cancel_prepared_work() {
    let f = Fixture::new();
    let release = f.release(|_| {});
    let body = serde_json::json!({"release":release})
        .to_string()
        .into_bytes();
    let (transport, server) = super::super::tests::serve_status(vec![
        (f.query(), 200, body.clone(), None),
        (f.query(), 200, body, None),
        (
            format!("/v1/mods/assets/{:x}", Sha256::digest(b"new")),
            200,
            b"new".to_vec(),
            None,
        ),
    ]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (outgoing, incoming) = crossbeam_channel::bounded(2);
    let worker = Worker {
        runtime: &runtime,
        transport: &transport,
        mods: f.root.path(),
        keys: Some(&f.keys),
        limit: 1000,
        guard: f.guard.clone(),
        generation: Arc::new(AtomicU64::new(1)),
        outgoing: &outgoing,
        deliveries: Deliveries::default(),
        refresh_at: u64::MAX,
    };
    let policy = Policy::default();
    let mut updates = Updates::default();
    let library = CinnabarModLibrary {
        xuid: "11".into(),
        mods: vec![f.receipt.selected.clone()],
    };
    let host = Host {
        revision: REVISION,
        api_revisions_json: &APIS,
    };
    updates.pass(
        &worker,
        1,
        &library,
        &host,
        &policy,
        || true,
        &|| 3000,
        &|_| {},
    );
    assert!(incoming.try_recv().is_err());
    policy.configure(true, false, []);
    updates.pass(
        &worker,
        1,
        &library,
        &host,
        &policy,
        || true,
        &|| 3000,
        &|_| {},
    );
    assert_eq!(policy.offers().len(), 1);
    assert!(incoming.try_recv().is_err());
    policy.configure(true, false, [policy.offers()[0].id]);
    updates.pass(
        &worker,
        1,
        &library,
        &host,
        &policy,
        || true,
        &|| 3001,
        &|_| {},
    );
    let prepared = incoming.try_recv().unwrap();
    policy.configure(false, false, []);
    assert!(prepared.install(3002, |_| Ok(())).is_err());
    assert_eq!(crate::mod_library::files::installed(f.root.path()).len(), 1);
    server.join().unwrap();
}

#[test]
fn malformed_query_response_backs_off_without_changing_files() {
    let f = Fixture::new();
    let (transport, server) =
        super::super::tests::serve_status(vec![(f.query(), 400, Vec::new(), None)]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (outgoing, incoming) = crossbeam_channel::bounded(2);
    let worker = Worker {
        runtime: &runtime,
        transport: &transport,
        mods: f.root.path(),
        keys: Some(&f.keys),
        limit: 1000,
        guard: f.guard.clone(),
        generation: Arc::new(AtomicU64::new(1)),
        outgoing: &outgoing,
        deliveries: Deliveries::default(),
        refresh_at: u64::MAX,
    };
    let policy = Policy::default();
    policy.configure(true, true, []);
    let mut updates = Updates::default();
    let library = CinnabarModLibrary {
        xuid: "11".into(),
        mods: vec![f.receipt.selected.clone()],
    };
    updates.pass(
        &worker,
        1,
        &library,
        &Host {
            revision: REVISION,
            api_revisions_json: &APIS,
        },
        &policy,
        || true,
        &|| 3000,
        &|_| {},
    );
    assert_eq!(updates.attempts[&f.path].retry_at, 3000 + RETRY_BASE_MS);
    updates.pass(
        &worker,
        1,
        &library,
        &Host {
            revision: REVISION,
            api_revisions_json: &APIS,
        },
        &policy,
        || panic!("backed off"),
        &|| 3001,
        &|_| {},
    );
    assert!(incoming.try_recv().is_err());
    assert_eq!(fs::read(&f.path).unwrap(), b"old");
    server.join().unwrap();
}

#[test]
fn update_verification_requires_a_newer_version_and_a_signed_supported_api() {
    let f = Fixture::new();
    for (api, version) in [
        (None, "1.1.0"),
        (Some("b".repeat(40)), "1.1.0"),
        (Some(REVISION.into()), "1.0.0"),
        (Some(REVISION.into()), "0.9.0"),
    ] {
        let release = f.release(|value| {
            value["api_revision"] = serde_json::json!(api);
            value["version"] = serde_json::json!(version);
        });
        assert!(
            release
                .verify_update(&f.keys, &f.receipt.selected, &[REVISION.into()], 1000, 3000)
                .is_err()
        );
    }
}

#[test]
fn successor_receipts_consume_original_sends_after_restart_and_local_removal() {
    let f = Fixture::new();
    let mut prepared = f.pass(f.release(|_| {}), Some(b"new".to_vec()), false);
    let target = prepared.remove(0).install(3000, |_| Ok(())).unwrap();
    fs::remove_file(&f.path).unwrap();
    storage::retire_receipt(&f.path, &f.guard).unwrap();
    let mut deliveries = Deliveries::default();
    deliveries.refresh(2, f.root.path(), Some(&f.keys), 1000, &f.guard, 3000);
    assert!(
        deliveries
            .requests
            .contains(&request("11", &f.receipt.selected))
    );
    fs::remove_file(target).unwrap();
    assert!(storage::removed(
        f.root.path(),
        "11",
        &f.receipt.selected,
        &f.keys,
        1000,
        3000
    ));
}

#[test]
fn disabling_automatic_install_during_preference_snapshot_never_installs() {
    let f = Fixture::new();
    let mut prepared = f.pass(f.release(|_| {}), Some(b"new".to_vec()), false);
    let policy = Policy::default();
    policy.configure(true, true, []);
    let (flags, authorization) = policy.snapshot(|| {
        policy.configure(true, false, []);
    });
    assert_ne!(
        flags & 2,
        0,
        "the pass began with automatic install enabled"
    );
    let mut download = prepared.remove(0);
    download.authorization = Some((policy.authorization.clone(), authorization));
    assert!(download.install(3000, |_| Ok(())).is_err());
    assert_eq!(crate::mod_library::files::installed(f.root.path()).len(), 1);
}

#[test]
fn interrupted_marketplace_migrations_resume_on_periodic_manual_and_restarted_passes() {
    use crate::mod_library::{
        marketplace,
        port::{Backend, Selection},
        tests::{Fixture as BackendFixture, Permit},
    };
    use launcher::mods::Action;
    use std::sync::atomic::AtomicBool;

    for mode in ["periodic", "manual", "restart"] {
        for failure in [
            "pause",
            "runtime",
            "settings",
            "settings-publish",
            "transfer",
            "settings-removal",
        ] {
            let f = Fixture::new();
            let config = f.root.path().join("config");
            fs::create_dir_all(&config).unwrap();
            BackendFixture::apply(Action::Enable(0), &config, Some(&f.path), "enable".into())
                .unwrap();
            let old_settings = BackendFixture::settings_companion(&f.path);
            if failure == "settings" {
                fs::create_dir(&old_settings).unwrap();
            } else {
                fs::write(&old_settings, b"saved choices").unwrap();
            }
            let mut prepared = f.pass(f.release(|_| {}), Some(b"new".to_vec()), false);
            let next_settings = BackendFixture::settings_companion(
                &f.root
                    .path()
                    .join(storage::filename("11", &prepared[0].receipt.selected).unwrap()),
            );
            if failure == "settings-publish" {
                fs::create_dir(&next_settings).unwrap();
            }
            if matches!(failure, "pause" | "transfer") {
                fs::write(config.join(format!("fail-{failure}")), b"").unwrap();
            }
            let mut acknowledgements = 0;
            let message = marketplace::install::<BackendFixture>(
                prepared.remove(0),
                &config,
                "interrupted".into(),
                3000,
                &mut |_| {
                    acknowledgements += 1;
                    if failure == "settings-removal" && acknowledgements == 2 {
                        fs::remove_file(&old_settings).unwrap();
                        fs::create_dir(&old_settings).unwrap();
                    }
                    if failure == "runtime" {
                        Err("injected retirement failure".into())
                    } else {
                        Ok(Permit(Arc::new(AtomicBool::new(true))))
                    }
                },
            );
            assert!(
                message.starts_with("Marketplace installation held"),
                "{message}"
            );
            let registered = BackendFixture::registered(&config);
            assert_eq!(
                registered.len(),
                1,
                "{mode}/{failure}: a version must remain enabled"
            );
            assert!(registered[0].enabled());
            assert!(registered[0].component().exists());
            assert!(f.guard.allows(registered[0].component(), 3000));
            assert_eq!(crate::mod_library::files::installed(f.root.path()).len(), 2);
            if failure == "settings-removal" {
                fs::write(
                    BackendFixture::settings_companion(registered[0].component()),
                    b"new successor choices",
                )
                .unwrap();
            }
            if matches!(failure, "pause" | "transfer") {
                fs::remove_file(config.join(format!("fail-{failure}"))).unwrap();
            }
            if matches!(
                failure,
                "pause" | "runtime" | "settings" | "settings-publish" | "transfer"
            ) {
                assert_eq!(registered[0].component(), f.path);
                assert!(
                    !config.join("paused").exists(),
                    "failed migration releases admission"
                );
            }
            if failure == "settings-publish" {
                fs::remove_dir(&next_settings).unwrap();
            }
            if matches!(failure, "settings" | "settings-removal") {
                fs::remove_dir(&old_settings).unwrap();
                fs::write(&old_settings, b"saved choices").unwrap();
            }
            if mode == "restart" && failure != "settings-removal" {
                BackendFixture::pause_marketplace(&config, &f.path, "interrupted-fence".into())
                    .unwrap();
            }
            let guard = if mode == "restart" {
                let guard = Arc::new(Guard::default());
                guard.configure(1);
                guard.account(1, "11".into());
                guard.revocations(super::super::super::verify::RevocationState {
                    issued_at: 1000,
                    expires_at: 100_000_000,
                    revoked: HashMap::new(),
                });
                guard
            } else {
                f.guard.clone()
            };
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            // No update metadata or asset request is needed to resume a published successor.
            let transport = Transport {
                client: reqwest::Client::new(),
                origin: "http://127.0.0.1:0".parse().unwrap(),
            };
            let (outgoing, incoming) = crossbeam_channel::bounded(2);
            let worker = Worker {
                runtime: &runtime,
                transport: &transport,
                mods: f.root.path(),
                keys: Some(&f.keys),
                limit: 1000,
                guard,
                generation: Arc::new(AtomicU64::new(1)),
                outgoing: &outgoing,
                deliveries: Deliveries::default(),
                refresh_at: u64::MAX,
            };
            let policy = Policy::default();
            let id = crate::mod_library::files::installed(f.root.path())
                .into_iter()
                .find(|(_, path)| path == &f.path)
                .unwrap()
                .0
                .id;
            policy.configure(true, mode != "manual", (mode == "manual").then_some(id));
            let mut updates = Updates {
                epoch: 1,
                ..Default::default()
            };
            if mode != "restart" {
                updates.attempts.insert(
                    f.path.clone(),
                    Failure {
                        count: 1,
                        retry_at: 3000 + RETRY_BASE_MS,
                    },
                );
            }
            let now = if mode == "periodic" {
                3000 + RETRY_BASE_MS
            } else {
                3001
            };
            updates.pass(
                &worker,
                1,
                &CinnabarModLibrary {
                    xuid: "11".into(),
                    mods: vec![f.receipt.selected.clone()],
                },
                &Host {
                    revision: REVISION,
                    api_revisions_json: &APIS,
                },
                &policy,
                || true,
                &|| now,
                &|message| panic!("resuming a migration needs no HTTP request: {message}"),
            );
            let retry = incoming
                .try_recv()
                .expect("unfinished retirement must be queued again");
            let message = marketplace::install::<BackendFixture>(
                retry,
                &config,
                "retry".into(),
                now,
                &mut |_| Ok(Permit(Arc::new(AtomicBool::new(true)))),
            );
            assert!(
                message.starts_with("Updated"),
                "{mode}/{failure}: {message}"
            );
            assert!(!f.path.exists());
            let registered = BackendFixture::registered(&config);
            assert_eq!(registered.len(), 1);
            assert!(registered[0].enabled());
            assert!(registered[0].component().exists());
            assert_eq!(
                fs::read(BackendFixture::settings_companion(
                    registered[0].component()
                ))
                .unwrap(),
                if failure == "settings-removal" {
                    b"new successor choices".as_slice()
                } else {
                    b"saved choices".as_slice()
                }
            );
            assert_eq!(
                storage::restore(f.root.path(), &f.keys, 1000, &worker.guard, 3001).len(),
                1
            );
        }
    }
}
