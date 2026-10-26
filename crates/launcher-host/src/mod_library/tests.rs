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
struct Fixture;

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
struct Permit(Arc<AtomicBool>);
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
    /// Accepts fixture versions and rejects a deliberately malformed package.
    fn validate_source(bytes: &[u8]) -> Result<(), String> {
        bytes
            .first()
            .is_some_and(u8::is_ascii_digit)
            .then_some(())
            .ok_or("invalid fixture".into())
    }
    /// Supplies the canonical metadata model at the package inspection boundary.
    fn update_metadata(bytes: &[u8], _: bool) -> Result<ModMetadata, String> {
        Ok(ModMetadata {
            version: Some(String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())?),
            update_url: GitHubModAsset::parse(SOURCE),
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
    /// Writes the fixture's removal request and retains the exact bytes for confirmation.
    fn forget(directory: &Path, _: &Path, request: String) -> Result<Vec<u8>, String> {
        fs::create_dir_all(directory).unwrap();
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
