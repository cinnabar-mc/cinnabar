use super::*;
use crate::modding::registration::selection;
use launcher_host::{
    mod_library::update::{PendingReload, install},
    mod_updates::Downloaded,
};

const SOURCE: &str = "https://github.com/example/zoom/releases/latest/download/zoom.wasm";

#[test]
fn revision_aware_install_requires_the_exact_promised_replacement_identity() {
    for matches in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("zoom.wasm");
        let identity = |revision: &str| {
            serde_json::json!({
                "source": SOURCE, "package": ui::mod_metadata::OFFICIAL_UPDATE_PACKAGE, "api_revision": "a".repeat(40), "revision": revision,
            })
            .to_string()
        };
        let mut original = component_without_revision("1.0.0", SOURCE, true);
        section(
            &mut original,
            mod_api::REVISION_UPDATE_SECTION,
            identity(&"a".repeat(40)).as_bytes(),
        );
        fs::write(&path, &original).unwrap();
        let mut next = component_without_revision("1.1.0", SOURCE, true);
        section(
            &mut next,
            mod_api::REVISION_UPDATE_SECTION,
            identity(&if matches { "b" } else { "c" }.repeat(40)).as_bytes(),
        );
        let mut download = Downloaded::fixture(
            installed(directory.path())[0].0.id,
            "1.1.0",
            "example/zoom",
            "zoom.wasm",
            &next,
        );
        download.revision_update =
            ui::mod_metadata::RevisionModUpdate::parse(identity(&"b".repeat(40)).as_bytes())
                .map(Box::new);
        assert_eq!(
            install::<AppBackend>(directory.path(), &download).is_ok(),
            matches
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            if matches { next } else { original }
        );
    }
}

/// Builds a valid host-world component with versioned, root-level update metadata.
fn component_without_revision(version: &str, source: &str, valid: bool) -> Vec<u8> {
    requesting_without_revision(
        version,
        source,
        valid,
        "{\"camera\":true,\"settings\":true}",
    )
}

/// Builds a component like [`component_without_revision`] that requests `grants`.
fn requesting_without_revision(version: &str, source: &str, valid: bool, grants: &str) -> Vec<u8> {
    let code = if valid {
        "(component (core module $m (func (export \"init\")) (func (export \"frame\"))) (core instance $i (instantiate $m)) (func (export \"init\") (canon lift (core func $i \"init\"))) (func (export \"frame\") (canon lift (core func $i \"frame\"))))"
    } else {
        "(component)"
    };
    let mut bytes = wat::parse_str(code).unwrap();
    for (name, data) in [
        ("version", version),
        (mod_api::UPDATE_URL_SECTION, source),
        (mod_host::REQUESTED_GRANTS_SECTION, grants),
    ] {
        section(&mut bytes, name, data.as_bytes());
    }
    bytes
}

/// Adds the official revision identity consumed by update admission.
fn component(version: &str, source: &str, valid: bool) -> Vec<u8> {
    requesting(
        version,
        source,
        valid,
        "{\"camera\":true,\"settings\":true}",
    )
}

/// Builds an updatable component like [`component`] that requests `grants`.
fn requesting(version: &str, source: &str, valid: bool, grants: &str) -> Vec<u8> {
    let mut bytes = requesting_without_revision(version, source, valid, grants);
    let revision = {
        use sha2::{Digest, Sha256};
        format!(
            "{:x}",
            Sha256::digest(version.trim_start_matches('v').as_bytes())
        )[..40]
            .to_owned()
    };
    section(&mut bytes, mod_api::REVISION_UPDATE_SECTION, serde_json::json!({"source":source, "package":ui::mod_metadata::OFFICIAL_UPDATE_PACKAGE,"revision":revision,"api_revision":"a".repeat(40)}).to_string().as_bytes());
    bytes
}

/// Appends a custom section without depending on a second production encoder.
fn section(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
    /// Encodes an unsigned LEB128 section length.
    fn length(mut value: usize, bytes: &mut Vec<u8>) {
        loop {
            let byte = (value & 127) as u8;
            value >>= 7;
            bytes.push(byte | if value == 0 { 0 } else { 128 });
            if value == 0 {
                break;
            }
        }
    }
    let mut payload = Vec::new();
    length(name.len(), &mut payload);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    bytes.push(0);
    length(payload.len(), bytes);
    bytes.extend(payload);
}

#[test]
fn automatic_install_preserves_registration_settings_and_existing_authority() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    fs::create_dir(&mods).unwrap();
    let path = mods.join("renamed.wasm");
    fs::write(&path, component("1.0.0", SOURCE, true)).unwrap();
    let config = directory.path().join("config");
    selection::select(&config, &path, true, "enabled".into()).unwrap();
    let before = selection::registered(&config).remove(0).entry;
    let companion = mod_host::settings_companion(&path);
    fs::write(&companion, b"{\"volume\":17}").unwrap();
    let new = component("1.1.0", SOURCE, true);
    let download = Downloaded::fixture(
        installed(&mods)[0].0.id,
        "v1.1.0",
        "example/zoom",
        "zoom.wasm",
        &new,
    );
    let message = install::<AppBackend>(&mods, &download).unwrap().message;
    assert!(message.contains("Updated renamed.wasm to v1.1.0"));
    assert_eq!(fs::read(&path).unwrap(), new);
    assert_eq!(selection::registered(&config)[0].entry, before);
    assert_eq!(fs::read(&companion).unwrap(), b"{\"volume\":17}");
    assert_eq!(fs::read_dir(&mods).unwrap().count(), 2);
}

#[test]
fn wrong_versions_sources_invalid_components_and_cancellation_leave_original_intact() {
    for (version, source, valid, cancel) in [
        ("1.0.0", SOURCE, true, false),
        ("1.0.1", SOURCE, true, false),
        (
            "1.1.0",
            "https://github.com/other/zoom/releases/latest/download/zoom.wasm",
            true,
            false,
        ),
        ("1.1.0", SOURCE, false, false),
        ("1.1.0", SOURCE, true, true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("zoom.wasm");
        let original = component("1.0.0", SOURCE, true);
        fs::write(&path, &original).unwrap();
        let download = Downloaded::fixture(
            installed(directory.path())[0].0.id,
            "1.1.0",
            "example/zoom",
            "zoom.wasm",
            &component(version, source, valid),
        );
        if cancel {
            download.cancel_fixture();
        }
        assert!(
            install::<AppBackend>(directory.path(), &download).is_err(),
            "{version}, {source}, {valid}, {cancel}"
        );
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn replaced_or_removed_sources_cannot_receive_a_stale_download() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("zoom.wasm");
    fs::write(&path, component("1.0.0", SOURCE, true)).unwrap();
    let download = Downloaded::fixture(
        installed(directory.path())[0].0.id,
        "1.1.0",
        "example/zoom",
        "zoom.wasm",
        &component("1.1.0", SOURCE, true),
    );
    fs::write(&path, b"replacement selected by the user").unwrap();
    assert!(install::<AppBackend>(directory.path(), &download).is_err());
    assert_eq!(
        fs::read(&path).unwrap(),
        b"replacement selected by the user"
    );
    fs::remove_file(&path).unwrap();
    assert!(install::<AppBackend>(directory.path(), &download).is_err());
    assert!(!path.exists());
}

#[test]
fn requested_update_worker_installs_and_acknowledges_completion_with_auto_install_disabled() {
    use super::{Command, run};
    use crossbeam_channel::unbounded;
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    fs::create_dir(&mods).unwrap();
    let path = mods.join("zoom.wasm");
    fs::write(&path, component("1.0.0", SOURCE, true)).unwrap();
    let id = installed(&mods)[0].0.id;
    let bytes = component("1.1.0", SOURCE, true);
    let download = Downloaded::fixture(id, "1.1.0", "example/zoom", "zoom.wasm", &bytes);
    let (commands, incoming) = unbounded();
    let (outgoing, published) = unbounded();
    let (finished, completion) = unbounded();
    commands.send(Command::Update(download, finished)).unwrap();
    drop(commands);
    run::<AppBackend>(
        &mods,
        &directory.path().join("config"),
        &incoming,
        &outgoing,
        crate::modding::RELOAD_INTERVAL,
        || Ok(None),
        |_| panic!("updates must not retire or remove the mod"),
    );
    completion.recv().unwrap();
    let states: Vec<_> = published.try_iter().collect();
    assert_eq!(
        states.iter().map(|state| state.busy).collect::<Vec<_>>(),
        [false, true, false]
    );
    assert_eq!(
        states[1].update_progress[0].phase,
        launcher::mods::UpdatePhase::Installing
    );
    assert_eq!(states[1].update_progress[0].id, id);
    assert!(states[2].update_progress.is_empty());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(states[2].message.contains("Updated zoom.wasm to 1.1.0"));
}

/// Publishes a status for the exact registration request currently awaiting runtime admission.
fn status(registration: &Path, state: &str) {
    use crate::modding::registration::{REGISTRATION_FILE, STATUS_FILE};
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(registration.join(REGISTRATION_FILE)).unwrap()).unwrap();
    fs::write(registration.join(STATUS_FILE), serde_json::to_vec(&serde_json::json!({
        "version": record["version"], "request_id": record["request_id"],
        "independent_registration": true, "client_pid": std::process::id(),
        "mods": [{"component": record["mods"][0]["component"], "state": state, "message": "fixture reload error", "reload": record["mods"][0]["reload"].as_u64().unwrap_or_default()}]
    })).unwrap()).unwrap();
}

#[test]
fn reloading_waits_for_the_new_runtime_ack_and_preserves_grants() {
    use super::scan;
    use crossbeam_channel::unbounded;
    use std::time::Instant;
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let registration = directory.path().join("config");
    fs::create_dir(&mods).unwrap();
    let path = mods.join("zoom.wasm");
    fs::write(&path, component("1.0.0", SOURCE, true)).unwrap();
    selection::select(&registration, &path, true, "before".into()).unwrap();
    let original = selection::registered(&registration).remove(0).entry;
    status(&registration, "loaded");
    let download = Downloaded::fixture(
        installed(&mods)[0].0.id,
        "1.1.0",
        "example/zoom",
        "zoom.wasm",
        &component("1.1.0", SOURCE, true),
    );
    let installed = install::<AppBackend>(&mods, &download).unwrap();
    let (finished, completion) = unbounded();
    let now = Instant::now();
    let pending = PendingReload::start::<AppBackend>(
        &mods,
        &registration,
        installed,
        "new-reload".into(),
        finished,
        now,
    )
    .unwrap()
    .unwrap();
    let waiting = scan(&mods, &registration, String::new());
    assert_eq!(waiting.active[0].status, launcher::mods::Status::Starting);
    assert!(pending.poll(&waiting, now).is_none());
    assert!(completion.try_recv().is_err());
    // Another mod's trap can republish an old loaded status under the new list request.
    status(&registration, "loaded");
    let status_path = registration.join(crate::modding::registration::STATUS_FILE);
    let mut old: serde_json::Value =
        serde_json::from_slice(&fs::read(&status_path).unwrap()).unwrap();
    old["mods"][0]["reload"] = original.reload.into();
    fs::write(&status_path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(
        pending
            .poll(&scan(&mods, &registration, String::new()), now)
            .is_none()
    );
    let current = selection::registered(&registration).remove(0).entry;
    assert_eq!(current.grants, original.grants);
    assert_eq!(current.reload, original.reload + 1);
    assert!(
        pending
            .poll(&waiting, now + std::time::Duration::from_secs(61))
            .unwrap()
            .is_err()
    );
    status(&registration, "reload-error");
    assert!(
        pending
            .poll(&scan(&mods, &registration, String::new()), now)
            .unwrap()
            .unwrap_err()
            .contains("reload failed")
    );
    for state in ["loaded", "next-join"] {
        status(&registration, state);
        assert!(
            pending
                .poll(&scan(&mods, &registration, String::new()), now)
                .unwrap()
                .unwrap()
                .contains("Updated zoom.wasm to 1.1.0")
        );
    }
    pending.finish();
    completion.recv().unwrap();
}

/// Legacy packet update sections cannot authorize replacing a local component.
#[test]
fn local_packet_update_sections_do_not_authorize_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("packet.wasm");
    let code = include_str!("../../../../../crates/mod-host/src/packet_mod/tests/guest.wat")
        .replace("$IMPORTS", "")
        .replace("$START", "");
    let mut original = wat::parse_str(&code).unwrap();
    section(&mut original, "version", b"1.0.0");
    section(
        &mut original,
        mod_api::UPDATE_URL_SECTION,
        SOURCE.as_bytes(),
    );
    fs::write(&path, &original).unwrap();
    let config = directory.path().join("config");
    selection::select(&config, &path, true, "enabled".into()).unwrap();
    let before = selection::registered(&config).remove(0).entry;
    let mut next = wat::parse_str(&code).unwrap();
    section(&mut next, "version", b"1.1.0");
    section(&mut next, mod_api::UPDATE_URL_SECTION, SOURCE.as_bytes());
    let download = Downloaded::fixture(
        installed(directory.path())[0].0.id,
        "1.1.0",
        "example/zoom",
        "zoom.wasm",
        &next,
    );
    assert!(install::<AppBackend>(directory.path(), &download).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(selection::registered(&config)[0].entry, before);
}

#[test]
fn an_update_requesting_a_new_permission_asks_for_review_without_granting_it() {
    let label = mod_host::ModGrants::CAPABILITIES
        .iter()
        .find(|capability| capability.name == "render_material")
        .unwrap()
        .label;
    for adds in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mods = directory.path().join("mods");
        let config = directory.path().join("config");
        fs::create_dir(&mods).unwrap();
        let path = mods.join("client.wasm");
        let old_request = r#"{"camera":true,"settings":true}"#;
        fs::write(&path, requesting("1.0.0", SOURCE, true, old_request)).unwrap();
        selection::select(&config, &path, true, "enabled".into()).unwrap();
        selection::allow_requested(&config, &path, "consent".into()).unwrap();
        let before = selection::registered(&config).remove(0).entry;
        let previous = scan(&mods, &config, String::new()).installed.remove(0);
        let new_request = if adds {
            r#"{"camera":true,"settings":true,"render_material":true}"#
        } else {
            old_request
        };
        let next = requesting("1.1.0", SOURCE, true, new_request);
        let download =
            Downloaded::fixture(previous.id, "1.1.0", "example/zoom", "zoom.wasm", &next);
        let mut installed = install::<AppBackend>(&mods, &download).unwrap();
        installed.review_new_requests::<AppBackend>(&previous, &config);
        assert_eq!(
            installed.message.contains("It requests new permissions"),
            adds,
            "{}",
            installed.message
        );
        assert_eq!(installed.message.contains(label), adds);
        assert_eq!(selection::registered(&config)[0].entry, before);
        let card = scan(&mods, &config, String::new()).installed.remove(0);
        let waiting: Vec<_> = card
            .awaiting_approval()
            .map(|index| card.permissions[index].name.as_str())
            .collect();
        assert_eq!(waiting, if adds { vec![label] } else { Vec::new() });
        selection::allow_requested(&config, &path, "approve".into()).unwrap();
        assert_eq!(
            selection::registered(&config)[0].entry.grants,
            serde_json::from_str::<mod_host::ModGrants>(new_request).unwrap()
        );
        let card = scan(&mods, &config, String::new()).installed.remove(0);
        assert_eq!(card.awaiting_approval().count(), 0);
    }
}
