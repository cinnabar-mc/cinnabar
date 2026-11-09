use super::*;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use bridge::contract::CinnabarModEntry;
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

struct Fixture {
    signer: Ed25519KeyPair,
    keys: Keys,
    selected: CinnabarModEntry,
    release: SignedRelease,
    bytes: Vec<u8>,
}

/// Builds an original throwaway signed component selector, never a production credential.
fn fixture() -> Fixture {
    let signer = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
    let keys = Keys::parse(
        serde_json::to_vec(&serde_json::json!({"ed25519":[STANDARD.encode(signer.public_key())]}))
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    let bytes = b"\0asm\x0d\0\x01\0".to_vec();
    let selected = CinnabarModEntry {
        slug: "fixture-mod".into(),
        version: "1.0.0".into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes: bytes.len() as u64,
        sent_at: "2026-10-23T00:00:00Z".into(),
        pulled: false,
    };
    let statement = serde_json::json!({"slug":selected.slug,"version":selected.version,"sha256":selected.sha256,"bytes":selected.bytes,"grants":{"hud":true},"approved_at":1000,"client_revision":"a".repeat(40),"policy":"fixture-policy","publication":"manual"}).to_string();
    let release = SignedRelease {
        signature: URL_SAFE_NO_PAD.encode(signer.sign(statement.as_bytes())),
        statement,
    };
    Fixture {
        signer,
        keys,
        selected,
        release,
        bytes,
    }
}

/// Signs freshness and withdrawal fixtures over their exact JSON bytes.
fn revocations(f: &Fixture, issued: u64, expires: u64, hashes: &[&str]) -> SignedRevocations {
    let body=serde_json::json!({"issued_at":issued,"expires_at":expires,"revoked":hashes.iter().map(|hash|serde_json::json!({"sha256":hash,"reason":"Synthetic withdrawal"})).collect::<Vec<_>>()} ).to_string();
    SignedRevocations {
        signature: URL_SAFE_NO_PAD.encode(f.signer.sign(body.as_bytes())),
        body,
    }
}

#[test]
fn signatures_bind_exact_metadata_selection_size_and_bytes() {
    let f = fixture();
    let manifest = f.release.verify(&f.keys, &f.selected, 1024, 2000).unwrap();
    verify::verify_bytes(&f.bytes, &manifest).unwrap();
    let mut wrong = f.selected.clone();
    wrong.version = "9.0.0".into();
    assert!(f.release.verify(&f.keys, &wrong, 1024, 2000).is_err());
    assert!(f.release.verify(&f.keys, &f.selected, 1, 2000).is_err());
    assert!(verify::verify_bytes(b"changed", &manifest).is_err());
    let mut tampered = f.release.clone();
    tampered.statement.push(' ');
    assert!(tampered.verify(&f.keys, &f.selected, 1024, 2000).is_err());
    assert!(Keys::parse(br#"{"ed25519":[]}"#).is_err());
}

#[test]
fn storage_keeps_distinct_releases_separate_even_when_their_component_bytes_match() {
    let f = fixture();
    let initial = storage::filename("11", &f.selected).unwrap();
    let mut another = f.selected.clone();
    another.version = "1.0.1".into();
    assert_ne!(initial, storage::filename("11", &another).unwrap());
    another = f.selected.clone();
    another.slug = "other-project".into();
    assert_ne!(initial, storage::filename("11", &another).unwrap());
}

#[test]
fn revocations_enforce_expiry_clock_bounds_and_restart_rollback_protection() {
    let f = fixture();
    let current = revocations(&f, 2000, 5000, &[&f.selected.sha256]);
    let state = current.verify(&f.keys, 3000, 1000).unwrap();
    assert!(state.revoked.contains_key(&f.selected.sha256));
    assert!(current.verify(&f.keys, 5000, 0).is_err());
    assert!(current.verify(&f.keys, 3000, 2001).is_err());
    assert!(
        revocations(&f, 900_000, 910_000, &[])
            .verify(&f.keys, 3000, 0)
            .is_err()
    );
    let restored = current.restore(&f.keys, 9000).unwrap();
    assert_eq!(restored.issued_at, 2000);
    assert_eq!(restored.expires_at, 5000);
    assert!(
        revocations(&f, 1000, 20_000, &[])
            .verify(&f.keys, 9000, restored.issued_at)
            .is_err()
    );
}

/// Supplies a complete staged transfer under a live synthetic account and revocation checkpoint.
fn prepared(root: &Path, f: &Fixture, guard: Arc<Guard>, generation: Arc<AtomicU64>) -> Downloaded {
    guard.configure(1);
    guard.account(1, "11".into());
    guard.revocations(
        revocations(f, 2000, 9000, &[])
            .verify(&f.keys, 3000, 0)
            .unwrap(),
    );
    let mut file = tempfile::NamedTempFile::new_in(root).unwrap();
    file.write_all(&f.bytes).unwrap();
    Downloaded {
        file,
        manifest: f.release.verify(&f.keys, &f.selected, 1024, 3000).unwrap(),
        receipt: storage::Receipt {
            owner: "11".into(),
            selected: f.selected.clone(),
            release: f.release.clone(),
            consumed: Vec::new(),
            migration: None,
        },
        generation,
        epoch: 1,
        guard,
        mods: root.into(),
        previous: None,
        authorization: None,
    }
}

/// Stages a verified synthetic release for `mods` under a live account whose signed evidence is
/// fresh at 3000 ms, returning it with its managed destination.
pub(crate) fn staged(mods: &Path) -> (Downloaded, std::path::PathBuf) {
    let f = fixture();
    let target = mods.join(storage::filename("11", &f.selected).unwrap());
    let download = prepared(
        mods,
        &f,
        Arc::new(Guard::default()),
        Arc::new(AtomicU64::new(1)),
    );
    (download, target)
}

#[test]
fn atomic_install_stays_disabled_and_retains_receipts_for_removal_and_restart() {
    let f = fixture();
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    let download = prepared(root.path(), &f, guard.clone(), Arc::new(AtomicU64::new(1)));
    let path = download.install(3000, |_| Ok(())).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), f.bytes);
    assert!(
        !root.path().join("local-mod.json").exists(),
        "installation must not enable a mod or grant permissions"
    );
    assert!(guard.allows(&path, 3000));
    assert!(guard.check(&path, b"tampered", 3000).is_err());
    assert!(!guard.allows(&path, 9000));
    guard.configure(2);
    guard.account(1, "11".into());
    assert!(
        !guard.allows(&path, 3000),
        "late account reply cannot revive authority"
    );
    let restored = Guard::default();
    assert_eq!(
        storage::restore(root.path(), &f.keys, 1024, &restored, 3000).len(),
        1
    );
    assert!(
        !restored.allows(&path, 3000),
        "disk receipts cannot establish a signed-in account"
    );
    std::fs::remove_file(&path).unwrap();
    assert!(storage::removed(
        root.path(),
        "11",
        &f.selected,
        &f.keys,
        1024,
        3000
    ));
    let mut later = f.selected.clone();
    later.sent_at = "2026-10-24T00:00:00Z".into();
    assert!(
        !storage::removed(root.path(), "11", &later, &f.keys, 1024, 3000),
        "a new website send must not be removed by an old local receipt"
    );
    assert_eq!(
        storage::restore(root.path(), &f.keys, 1024, &restored, 3000).len(),
        1,
        "local removal remains consumed until another website send"
    );
}

#[test]
fn cancelled_downloads_and_existing_file_collisions_never_publish() {
    let f = fixture();
    let root = tempfile::tempdir().unwrap();
    let guard = Arc::new(Guard::default());
    let generation = Arc::new(AtomicU64::new(1));
    let download = prepared(root.path(), &f, guard.clone(), generation.clone());
    generation.store(2, Ordering::SeqCst);
    assert!(download.install(3000, |_| Ok(())).is_err());
    let download = prepared(root.path(), &f, guard.clone(), Arc::new(AtomicU64::new(1)));
    let target = root
        .path()
        .join(storage::filename("11", &f.selected).unwrap());
    std::fs::write(&target, b"another file").unwrap();
    assert!(download.install(3000, |_| Ok(())).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"another file");
}

#[test]
fn withdrawals_remain_held_and_unsigned_local_imports_keep_their_existing_path() {
    let f = fixture();
    let guard = Guard::default();
    let name = format!("{}11-fixture.wasm", launcher::mods::MARKETPLACE_FILE_PREFIX);
    let path = Path::new(&name);
    guard.configure(1);
    guard.account(1, "11".into());
    guard.installed(
        path.into(),
        "11".into(),
        f.release.verify(&f.keys, &f.selected, 1024, 3000).unwrap(),
    );
    guard.revocations(
        revocations(&f, 2000, 9000, &[&f.selected.sha256])
            .verify(&f.keys, 3000, 0)
            .unwrap(),
    );
    guard.revocations(verify::RevocationState {
        issued_at: 3000,
        expires_at: 9000,
        revoked: HashMap::new(),
    });
    assert!(!guard.allows(path, 3000));
    assert!(guard.reason(path, 3000).contains("Synthetic withdrawal"));
    assert!(!guard.allows(
        Path::new(&format!(
            "{}no-receipt.wasm",
            launcher::mods::MARKETPLACE_FILE_PREFIX
        )),
        3000
    ));
    assert!(guard.check(Path::new("local.wasm"), b"local", 3000).is_ok());
    assert!(storage::filename("../outside", &f.selected).is_err());
}

#[test]
fn only_reserved_names_are_managed_so_marketplace_named_local_mods_run_unheld() {
    let f = fixture();
    let guard = Guard::default();
    let local = Path::new("marketplace-prices.wasm");
    assert!(!managed(local));
    assert!(guard.allows(local, 3000));
    assert!(matches!(guard.check(local, b"local", 3000), Ok(None)));
    let stored = storage::filename("11", &f.selected).unwrap();
    assert!(managed(Path::new(&stored)), "{stored}");
    assert!(!guard.allows(Path::new(&stored), 3000));
}

/// Stages an update with signed grant changes, sharing its installed predecessor's account fence.
pub(crate) fn staged_update(
    mods: &Path,
    grants: serde_json::Value,
) -> (Downloaded, std::path::PathBuf) {
    let mut f = fixture();
    let guard = Arc::new(Guard::default());
    let generation = Arc::new(AtomicU64::new(1));
    let previous = prepared(mods, &f, guard.clone(), generation.clone())
        .install(3000, |_| Ok(()))
        .unwrap();
    let consumed = f.selected.clone();
    f.selected.version = "1.1.0".into();
    let mut statement: serde_json::Value = serde_json::from_str(&f.release.statement).unwrap();
    statement["version"] = serde_json::json!(f.selected.version);
    statement["grants"] = grants;
    statement["api_revision"] = serde_json::json!("a".repeat(40));
    f.release.statement = statement.to_string();
    f.release.signature = URL_SAFE_NO_PAD.encode(f.signer.sign(f.release.statement.as_bytes()));
    let mut download = prepared(mods, &f, guard, generation);
    download.previous = Some(previous.clone());
    download.receipt.consumed = vec![consumed];
    (download, previous)
}

#[test]
fn account_change_during_registration_release_cancels_publication() {
    let root = tempfile::tempdir().unwrap();
    let (download, target) = staged(root.path());
    let guard = download.guard();
    assert!(
        download
            .install(3000, |_| {
                guard.configure(2);
                Ok(())
            })
            .is_err()
    );
    assert!(!target.exists());
}
