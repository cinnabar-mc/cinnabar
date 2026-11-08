use super::{
    Command, Service,
    files::{import, installed},
    port::{Backend, Retirement, Selection},
    scan::scan_cached,
    update, worker,
};
use crossbeam_channel::unbounded;
use launcher::mods::{Action, Library};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use ui::mod_metadata::{GitHubModAsset, ModMetadata};

const SOURCE: &str = "https://github.com/example/zoom/releases/latest/download/zoom.wasm";

/// A lightweight package authority for testing service behavior without a guest engine.
pub(crate) struct Fixture;

impl Selection for (PathBuf, launcher::mods::Status) {
    /// Exposes the fixture's source identity without a registration schema.
    fn component(&self) -> &Path {
        &self.0
    }
    /// Fixture registrations contain only enabled sources.
    fn enabled(&self) -> bool {
        true
    }
    /// Supplies the fixture's runtime acknowledgement.
    fn status(&self) -> launcher::mods::Status {
        self.1.clone()
    }
}

/// A retirement acknowledgement that can be revoked before deletion.
pub(crate) struct Permit(pub(crate) Arc<AtomicBool>);
impl Retirement for Permit {
    /// Rejects revocation at the same deletion boundary as the runtime port.
    fn confirm(&self) -> Result<(), String> {
        self.0
            .load(Ordering::Acquire)
            .then_some(())
            .ok_or("superseded".into())
    }
    /// This fixture has no surviving settings writers.
    fn shared_companion(&self, _: &Path) -> bool {
        false
    }
}

impl Backend for Fixture {
    type Registered = (PathBuf, launcher::mods::Status);
    type Declarations = String;
    type Receipt = Vec<u8>;
    type Permit = Permit;
    const MAX_SOURCE_BYTES: usize = 100;

    /// Reads a plain fixture path, independent of the real registration schema.
    fn registered(directory: &Path) -> Vec<Self::Registered> {
        let path = fs::read_to_string(directory.join("selection")).unwrap_or_default();
        if path.is_empty() {
            Vec::new()
        } else {
            vec![(path.into(), launcher::mods::Status::Starting)]
        }
    }
    /// Uses file contents to detect cache hits and revision invalidation.
    fn declarations(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }
    /// Attaches only fixture metadata; grant projection is tested with the real app port.
    fn describe(
        item: &mut launcher::mods::InstalledMod,
        declared: &String,
        _: Option<&Self::Registered>,
    ) {
        item.metadata.version = Some(declared.clone());
    }
    /// Reads a test package without needing a guest engine.
    fn read_source(path: &Path) -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|error| error.to_string())
    }
    /// Accepts fixture versions and component bytes, and rejects a deliberately malformed package.
    fn validate_source(bytes: &[u8]) -> Result<(), String> {
        (bytes.first().is_some_and(u8::is_ascii_digit)
            || bytes.starts_with(b"\0asm")
            || matches!(bytes, b"old" | b"new"))
        .then_some(())
        .ok_or("invalid fixture".into())
    }
    /// Accepts any verified release; declaration matching is tested with the real app port.
    fn validate_marketplace(_: &[u8], _: &crate::marketplace::Manifest) -> Result<(), String> {
        Ok(())
    }
    /// Uses a single synthetic capability to exercise migration without importing the runtime schema.
    fn marketplace_subset(
        old: &crate::marketplace::Manifest,
        next: &crate::marketplace::Manifest,
    ) -> Result<bool, String> {
        Ok(next.grants == old.grants || next.grants == serde_json::json!({}))
    }
    /// Retains enabled selection while a deterministic runtime observes the admission fence.
    fn pause_marketplace(
        directory: &Path,
        previous: &Path,
        request: String,
    ) -> Result<Vec<u8>, String> {
        if directory.join("fail-pause").exists() {
            return Err("injected pause failure".into());
        }
        fs::write(directory.join("paused"), previous.to_str().unwrap())
            .map_err(|e| e.to_string())?;
        fs::write(directory.join("receipt"), &request).map_err(|e| e.to_string())?;
        Ok(request.into_bytes())
    }
    /// Reopens predecessor admission after a failed settings handoff.
    fn resume_marketplace(directory: &Path, receipt: &Vec<u8>) -> Result<(), String> {
        Self::confirm(receipt, directory)?;
        fs::remove_file(directory.join("paused")).map_err(|e| e.to_string())
    }
    /// Migrates the fixture's existing enabled path, leaving absent registrations disabled.
    fn replace_marketplace(
        directory: &Path,
        old: Option<&Self::Registered>,
        next: &Path,
        _: &crate::marketplace::Manifest,
        _: String,
    ) -> Result<(), String> {
        if old.is_some() {
            if directory.join("fail-transfer").exists() {
                return Err("injected transfer failure".into());
            }
            fs::write(directory.join("selection"), next.to_str().unwrap()).unwrap();
            let _ = fs::remove_file(directory.join("paused"));
            // Model immediate admission and an init save at the registration publication boundary.
            let settings =
                fs::read(Self::settings_companion(next)).unwrap_or_else(|_| b"defaults".to_vec());
            fs::write(directory.join("started-settings"), &settings).unwrap();
            if directory.join("save-on-start").exists() {
                fs::write(Self::settings_companion(next), b"successor save").unwrap();
            }
        }
        Ok(())
    }
    /// Supplies the canonical metadata model at the package inspection boundary.
    fn update_metadata(bytes: &[u8], _: bool) -> Result<ModMetadata, String> {
        Ok(ModMetadata {
            version: Some(String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())?),
            revision_update: Some(ui::mod_metadata::RevisionModUpdate {
                source: GitHubModAsset::parse(SOURCE).unwrap(),
                package: ui::mod_metadata::OFFICIAL_UPDATE_PACKAGE.into(),
                revision: {
                    use sha2::{Digest, Sha256};
                    format!("{:x}", Sha256::digest(bytes))[..40].into()
                },
                api_revision: "a".repeat(40),
            }),
            ..Default::default()
        })
    }
    /// Persists a selected path so a fresh service observation sees the command.
    fn apply(
        action: Action,
        directory: &Path,
        component: Option<&Path>,
        _: String,
    ) -> Result<(), String> {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let value = match action {
            Action::Enable(_) => component.unwrap().to_str().unwrap(),
            _ => "",
        };
        fs::write(directory.join("selection"), value).map_err(|error| error.to_string())
    }
    /// Drops the selection naming `component`, writes the fixture's removal request and retains
    /// the exact bytes for confirmation.
    fn forget(directory: &Path, component: &Path, request: String) -> Result<Vec<u8>, String> {
        fs::create_dir_all(directory).unwrap();
        if Self::registered(directory)
            .iter()
            .any(|(path, _)| path == component)
        {
            fs::write(directory.join("selection"), "").unwrap();
        }
        fs::write(directory.join("receipt"), &request).unwrap();
        Ok(request.into_bytes())
    }
    /// Rejects a receipt changed while retirement was pending.
    fn confirm(receipt: &Vec<u8>, directory: &Path) -> Result<(), String> {
        (fs::read(directory.join("receipt")).unwrap_or_default() == *receipt)
            .then_some(())
            .ok_or("selection changed".into())
    }
    /// Gives the fixture a stable companion without repeating runtime filename policy.
    fn settings_companion(component: &Path) -> PathBuf {
        component.with_extension("fixture-settings")
    }
}

#[test]
fn marketplace_navigation_does_not_modify_the_local_library() {
    let root = tempfile::tempdir().unwrap();
    let mut requests = 0;
    let result = worker::perform::<Fixture>(
        Action::BrowseMarketplace,
        &root.path().join("mods"),
        &root.path().join("config"),
        &mut requests,
        &mut || panic!("navigation must not open a file picker"),
        &mut |_| panic!("navigation must not retire a mod"),
    );
    assert_eq!(result, Ok(String::new()));
    assert_eq!(requests, 0);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn scanning_caches_revisions_and_persisted_selection_survives_a_fresh_scan() {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let config = root.path().join("config");
    fs::create_dir(&mods).unwrap();
    let path = mods.join("zoom.wasm");
    fs::write(&path, b"1.0.0").unwrap();
    fs::write(mods.join(".hidden.wasm"), b"hidden").unwrap();
    fs::write(mods.join("staging.tmp"), b"partial").unwrap();
    let mut cache = BTreeMap::new();
    let first = scan_cached::<Fixture>(&mods, &config, String::new(), &mut cache);
    assert_eq!(first.installed.len(), 1);
    let id = first.installed[0].id;
    cache.insert(id, "cached declaration".into());
    let cached = scan_cached::<Fixture>(&mods, &config, String::new(), &mut cache);
    assert_eq!(
        cached.installed[0].metadata.version.as_deref(),
        Some("cached declaration")
    );
    worker::perform::<Fixture>(
        Action::Enable(id),
        &mods,
        &config,
        &mut 0,
        &mut || panic!("no picker"),
        &mut |_| panic!("no retirement"),
    )
    .unwrap();
    let fresh = scan_cached::<Fixture>(&mods, &config, String::new(), &mut BTreeMap::new());
    assert_eq!(fresh.active[0].id, id);
    fs::write(&path, b"2.0.0-longer").unwrap();
    let changed = scan_cached::<Fixture>(&mods, &config, String::new(), &mut cache);
    assert_ne!(changed.installed[0].id, id);
    assert_eq!(
        changed.installed[0].metadata.version.as_deref(),
        Some("2.0.0-longer")
    );
    assert_eq!(cache.len(), 1);
}

#[test]
fn imports_validate_before_publication_and_never_replace_existing_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("zoom.wasm");
    let mods = root.path().join("mods");
    fs::write(&source, b"invalid").unwrap();
    assert!(import::<Fixture>(&mods, &source).is_err());
    assert!(!mods.exists());
    fs::write(&source, b"1.0.0").unwrap();
    assert_eq!(import::<Fixture>(&mods, &source).unwrap(), "zoom.wasm");
    fs::write(&source, b"2.0.0").unwrap();
    assert!(import::<Fixture>(&mods, &source).is_err());
    assert_eq!(fs::read(mods.join("zoom.wasm")).unwrap(), b"1.0.0");
}

#[test]
fn resent_marketplace_release_drops_a_stale_enabled_entry_and_lands_in_my_mods() {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let config = root.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    fs::create_dir_all(&config).unwrap();
    let (download, target) = crate::marketplace::tests::staged(&mods);
    // The earlier copy was enabled, then its file was deleted outside Mods.
    fs::write(config.join("selection"), target.to_str().unwrap()).unwrap();
    let message = super::marketplace::install::<Fixture>(
        download,
        &config,
        "resend".into(),
        3000,
        &mut |_| Ok(Permit(Arc::new(AtomicBool::new(true)))),
    );
    assert!(message.contains("My mods"), "{message}");
    assert!(target.exists(), "{message}");
    let library = scan_cached::<Fixture>(&mods, &config, message, &mut BTreeMap::new());
    assert_eq!(library.installed.len(), 1);
    assert!(
        library.active.is_empty(),
        "a re-sent release must wait for a fresh Enable"
    );
}

#[test]
fn local_files_named_like_the_marketplace_import_and_update_while_reserved_names_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let source = root.path().join("marketplace-prices.wasm");
    fs::write(&source, b"1.0.0").unwrap();
    assert_eq!(
        import::<Fixture>(&mods, &source).unwrap(),
        "marketplace-prices.wasm"
    );
    let id = installed(&mods)[0].0.id;
    let download =
        crate::mod_updates::Downloaded::fixture(id, "1.1.0", "example/zoom", "zoom.wasm", b"1.1.0");
    update::install::<Fixture>(&mods, &download).unwrap();
    assert_eq!(
        fs::read(mods.join("marketplace-prices.wasm")).unwrap(),
        b"1.1.0"
    );
    let reserved = root.path().join(format!(
        "{}prices.wasm",
        launcher::mods::MARKETPLACE_FILE_PREFIX
    ));
    fs::write(&reserved, b"1.0.0").unwrap();
    assert!(import::<Fixture>(&mods, &reserved).is_err());
}

#[test]
fn removal_checks_selection_and_retirement_before_deleting_files() {
    for changed_receipt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("zoom.wasm");
        let companion = Fixture::settings_companion(&path);
        fs::write(&path, b"1.0.0").unwrap();
        fs::write(&companion, b"preferences").unwrap();
        let id = installed(root.path())[0].0.id;
        let result = worker::perform::<Fixture>(
            Action::Remove(id),
            root.path(),
            root.path(),
            &mut 0,
            &mut || panic!("no picker"),
            &mut |_| {
                if changed_receipt {
                    fs::write(root.path().join("receipt"), b"changed").unwrap();
                }
                Ok(Permit(Arc::new(AtomicBool::new(changed_receipt))))
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"1.0.0");
        assert_eq!(fs::read(&companion).unwrap(), b"preferences");
    }
}

#[test]
fn failed_updates_preserve_original_bytes_and_success_waits_for_reload() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("zoom.wasm");
    fs::write(&path, b"1.0.0").unwrap();
    let id = installed(root.path())[0].0.id;
    for version in ["0.9.0", "invalid", "2.0.0"] {
        let download = crate::mod_updates::Downloaded::fixture(
            id,
            "1.1.0",
            "example/zoom",
            "zoom.wasm",
            version.as_bytes(),
        );
        assert!(update::install::<Fixture>(root.path(), &download).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"1.0.0");
    }
    let download =
        crate::mod_updates::Downloaded::fixture(id, "1.1.0", "example/zoom", "zoom.wasm", b"1.1.0");
    let applied = update::install::<Fixture>(root.path(), &download).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"1.1.0");
    fs::write(root.path().join("selection"), path.to_str().unwrap()).unwrap();
    let (finished, completion) = unbounded();
    let now = Instant::now();
    let pending = update::PendingReload::start::<Fixture>(
        root.path(),
        root.path(),
        applied,
        "reload".into(),
        finished,
        now,
    )
    .unwrap()
    .unwrap();
    let mut snapshot = scan_cached::<Fixture>(
        root.path(),
        root.path(),
        String::new(),
        &mut BTreeMap::new(),
    );
    // Fixture reload writes no active entry; supply the runtime's pending acknowledgement.
    snapshot.active = vec![launcher::mods::ActiveMod {
        id: pending.id,
        status: launcher::mods::Status::Starting,
    }];
    assert!(pending.poll(&snapshot, now).is_none());
    assert!(completion.try_recv().is_err());
    snapshot.active[0].status = launcher::mods::Status::ReloadFailed("activation refused".into());
    assert!(pending.poll(&snapshot, now).unwrap().is_err());
    pending.finish();
    completion.recv().unwrap();
}

#[test]
fn service_publishes_in_the_same_drive_and_retains_unchanged_shared_snapshots() {
    let (commands, incoming) = unbounded();
    let (outgoing, published) = unbounded();
    let mut service = Service {
        commands,
        published,
        updates: None,
        marketplace_offers: Vec::new(),
        checks: false,
        automatic: false,
        busy: false,
        progress: Vec::new(),
        installing: Vec::new(),
    };
    let mut snapshot = Arc::new(launcher::mods::Snapshot::default());
    let retained = snapshot.clone();
    let original = (*retained).clone();
    service.drive(Vec::new(), &mut snapshot, false, false);
    assert!(Arc::ptr_eq(&snapshot, &retained));
    assert_eq!(*snapshot, original);
    service.drive(vec![Action::ReloadAll], &mut snapshot, false, false);
    assert!(Arc::ptr_eq(&snapshot, &retained));
    assert!(matches!(
        incoming.try_recv(),
        Ok(Command::Action(Action::ReloadAll))
    ));
    outgoing
        .send(Library {
            message: "old".into(),
            ..Default::default()
        })
        .unwrap();
    outgoing
        .send(Library {
            message: "latest".into(),
            busy: true,
            ..Default::default()
        })
        .unwrap();
    service.drive(vec![Action::ReloadAll], &mut snapshot, false, false);
    assert_eq!(snapshot.library.message, "latest");
    assert!(!Arc::ptr_eq(&snapshot, &retained));
    assert_eq!(*retained, original);
    assert!(snapshot.library.busy);
    assert!(matches!(
        incoming.try_recv(),
        Ok(Command::Action(Action::ReloadAll))
    ));
}

#[test]
fn cancellation_publishes_idle_before_the_next_command() {
    let root = tempfile::tempdir().unwrap();
    let (commands, incoming) = unbounded();
    let (outgoing, published) = unbounded();
    commands.send(Command::Action(Action::Import)).unwrap();
    commands.send(Command::Action(Action::Import)).unwrap();
    drop(commands);
    worker::run::<Fixture>(
        root.path(),
        root.path(),
        &incoming,
        &outgoing,
        Duration::from_secs(1),
        || Ok(None),
        |_| panic!("no retirement"),
    );
    let states: Vec<_> = published.try_iter().collect();
    assert_eq!(
        states.iter().map(|state| state.busy).collect::<Vec<_>>(),
        [false, true, false, true, false]
    );
}

#[test]
fn checker_idle_and_unknown_update_actions_preserve_shared_snapshot_identity() {
    let (commands, _) = unbounded();
    let (_, published) = unbounded();
    let mut service = Service {
        commands,
        published,
        updates: Some(crate::mod_updates::tests::service_fixture(None)),
        marketplace_offers: Vec::new(),
        checks: false,
        automatic: false,
        busy: false,
        progress: Vec::new(),
        installing: Vec::new(),
    };
    let mut snapshot = Arc::new(launcher::mods::Snapshot::default());
    let retained = snapshot.clone();
    service.drive(vec![Action::Update(7)], &mut snapshot, false, false);
    assert!(Arc::ptr_eq(&snapshot, &retained));
    assert_eq!(*snapshot, *retained);
    service.updates = Some(crate::mod_updates::tests::service_fixture(Some(
        "update unavailable".into(),
    )));
    service.drive(Vec::new(), &mut snapshot, false, false);
    assert_eq!(snapshot.library.message, "update unavailable");
    assert!(retained.library.message.is_empty());
    assert!(!Arc::ptr_eq(&snapshot, &retained));
}

#[test]
fn marketplace_same_or_fewer_grants_replace_retired_files_and_keep_enabled_state() {
    for (enabled, grants) in [
        (true, serde_json::json!({"hud":true})),
        (true, serde_json::json!({})),
        (false, serde_json::json!({"hud":true})),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mods = root.path().join("mods");
        let config = root.path().join("config");
        fs::create_dir_all(&mods).unwrap();
        fs::create_dir_all(&config).unwrap();
        let (download, old) = crate::marketplace::tests::staged_update(&mods, grants);
        let guard = download.guard();
        if enabled {
            Fixture::apply(Action::Enable(0), &config, Some(&old), "enable".into()).unwrap();
        }
        let companion = Fixture::settings_companion(&old);
        fs::write(&companion, b"saved choices").unwrap();
        let mut retired = false;
        let message = super::marketplace::install::<Fixture>(
            download,
            &config,
            "replace".into(),
            3000,
            &mut |_| {
                retired = true;
                Ok(Permit(Arc::new(AtomicBool::new(true))))
            },
        );
        assert!(message.starts_with("Updated"), "{message}");
        assert!(retired);
        assert!(!old.exists());
        assert!(!guard.allows(&old, 3000));
        let files = installed(&mods);
        assert_eq!(files.len(), 1);
        assert_eq!(Fixture::registered(&config).len(), usize::from(enabled));
        if enabled {
            assert_eq!(Fixture::registered(&config)[0].component(), files[0].1);
        }
        assert_eq!(
            fs::read(Fixture::settings_companion(&files[0].1)).unwrap(),
            b"saved choices"
        );
        let receipts: Vec<_> = fs::read_dir(mods.join(".marketplace"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(receipts.len(), 1, "only the successor receipt remains");
    }
}

#[test]
fn marketplace_expanded_grants_keep_the_old_version_until_the_new_one_is_enabled() {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let config = root.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    fs::create_dir_all(&config).unwrap();
    let (download, old) = crate::marketplace::tests::staged_update(
        &mods,
        serde_json::json!({"hud":true,"camera":true}),
    );
    let guard = download.guard();
    Fixture::apply(Action::Enable(0), &config, Some(&old), "enable".into()).unwrap();
    let message = super::marketplace::install::<Fixture>(
        download,
        &config,
        "expanded".into(),
        3000,
        &mut |_| panic!("old version must keep running"),
    );
    assert!(message.contains("needs permission review"), "{message}");
    assert_eq!(Fixture::registered(&config)[0].component(), old);
    assert!(old.exists());
    assert_eq!(installed(&mods).len(), 2);
    let (_, next) = installed(&mods)
        .into_iter()
        .find(|(_, path)| path != &old)
        .unwrap();
    assert_eq!(guard.predecessors(&next, 3000), vec![old.clone()]);
    super::marketplace::enable::<Fixture>(
        &next,
        &config,
        "enable-new".into(),
        &guard,
        3000,
        &mut |_| Ok(Permit(Arc::new(AtomicBool::new(true)))),
    )
    .unwrap();
    assert!(!old.exists());
    assert_eq!(Fixture::registered(&config)[0].component(), next);
}

#[test]
fn removed_predecessors_do_not_block_enabling_an_expanded_update() {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let config = root.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    fs::create_dir_all(&config).unwrap();
    let (download, old) = crate::marketplace::tests::staged_update(
        &mods,
        serde_json::json!({"hud":true,"camera":true}),
    );
    let guard = download.guard();
    super::marketplace::install::<Fixture>(download, &config, "expanded".into(), 3000, &mut |_| {
        panic!("no retirement before Enable")
    });
    fs::remove_file(&old).unwrap();
    let (_, next) = installed(&mods).pop().unwrap();
    super::marketplace::enable::<Fixture>(
        &next,
        &config,
        "enable".into(),
        &guard,
        3000,
        &mut |_| panic!("the old source is already gone"),
    )
    .unwrap();
    assert_eq!(Fixture::registered(&config)[0].component(), next);
}

/// Runs an update with an immediate successor admission and optional init save.
fn settings_update(save_on_start: bool) {
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let config = root.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    fs::create_dir_all(&config).unwrap();
    let (download, old) =
        crate::marketplace::tests::staged_update(&mods, serde_json::json!({"hud":true}));
    Fixture::apply(Action::Enable(0), &config, Some(&old), "enable".into()).unwrap();
    fs::write(Fixture::settings_companion(&old), b"previous disk save").unwrap();
    if save_on_start {
        fs::write(config.join("save-on-start"), b"").unwrap();
    }
    let mut acknowledgements = 0;
    let message = super::marketplace::install::<Fixture>(
        download,
        &config,
        "replace".into(),
        3000,
        &mut |_| {
            acknowledgements += 1;
            if acknowledgements == 1 {
                fs::write(Fixture::settings_companion(&old), b"exact final settings").unwrap();
                let next = installed(&mods)
                    .into_iter()
                    .find(|(_, path)| path != &old)
                    .unwrap()
                    .1;
                // An unfinished prior admission may already have written destination defaults.
                if config.join("paused").exists() {
                    fs::write(Fixture::settings_companion(&next), b"stale successor save").unwrap();
                }
            }
            Ok(Permit(Arc::new(AtomicBool::new(true))))
        },
    );
    assert!(message.starts_with("Updated"), "{message}");
    let next = Fixture::registered(&config).remove(0).0;
    assert_eq!(
        fs::read(Fixture::settings_companion(&next)).unwrap(),
        if save_on_start {
            b"successor save".as_slice()
        } else {
            b"exact final settings".as_slice()
        }
    );
    assert_eq!(
        fs::read(config.join("started-settings")).unwrap(),
        b"exact final settings"
    );
    assert!(!Fixture::settings_companion(&old).exists());
    assert!(!old.exists());
}

#[test]
fn marketplace_successor_starts_with_final_predecessor_settings() {
    settings_update(false);
}

#[test]
fn marketplace_successor_init_save_cannot_discard_predecessor_settings() {
    settings_update(true);
}
