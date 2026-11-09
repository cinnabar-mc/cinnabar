//! The exact browser/API component enters the existing Available, enable and consent controls.

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

#[test]
fn website_component_is_available_without_grants_then_runs_with_explicit_consent() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../crates/launcher-host/src/marketplace/fixtures/website-send.json"
    ))
    .unwrap();
    let bytes = STANDARD
        .decode(fixture["artifact"].as_str().unwrap())
        .unwrap();
    let response: serde_json::Value =
        serde_json::from_str(fixture["release_response"].as_str().unwrap()).unwrap();
    let manifest: launcher_host::marketplace::Manifest =
        serde_json::from_str(response["release"]["statement"].as_str().unwrap()).unwrap();
    mod_host::validate_source(&bytes).unwrap();
    crate::modding::marketplace::declarations(&bytes, &manifest).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    let registration = root.path().join("config");
    fs::create_dir_all(&mods).unwrap();
    let path = mods.join(format!(
        "{}synthetic.wasm",
        launcher::mods::MARKETPLACE_FILE_PREFIX
    ));
    fs::write(&path, &bytes).unwrap();
    let library = scan(&mods, &registration, String::new());
    assert!(library.active.is_empty());
    assert!(registered(&registration).is_empty());
    let item = &library.installed[0];
    let id = item.id;
    let hud = item
        .permissions
        .iter()
        .find(|permission| permission.name == "HUD")
        .unwrap();
    assert_eq!(hud.requested, Some(true));
    assert!(!hud.enabled);
    let mut requests = 0;
    let mut act = |action| {
        perform::<AppBackend>(
            action,
            &mods,
            &registration,
            &mut requests,
            &mut || Ok(None),
            &mut |_| Ok(retirement_permit()),
        )
        .unwrap()
    };
    act(Action::Enable(id));
    assert_eq!(
        registered(&registration)[0].entry.grants,
        mod_host::ModGrants::default()
    );
    let capability = mod_host::ModGrants::CAPABILITIES
        .iter()
        .position(|value| value.name == "hud")
        .unwrap();
    act(Action::Permission(id, capability, true));
    let granted = registered(&registration)[0].entry.grants.clone();
    assert!(granted.hud);
    let mut host = ModHost::load_snapshot_with_grants(&path, &bytes, granted).unwrap();
    host.frame(false).unwrap();
    assert_eq!(host.label(), Some("Hello"));
    act(Action::Disable(id));
    assert!(!registered(&registration)[0].entry.enabled);
}

#[test]
fn signed_update_grants_compare_command_values_and_migrate_only_retained_choices() {
    use launcher_host::mod_library::port::Backend;
    let manifest = |grants| launcher_host::marketplace::Manifest {
        slug: "fixture".into(),
        version: "1.0.0".into(),
        sha256: "a".repeat(registration::MAX_REQUEST_BYTES),
        bytes: 1,
        grants,
        approved_at: 1,
        client_revision: "a".repeat(40),
        api_revision: None,
        policy: "fixture".into(),
        publication: "automatic".into(),
    };
    let old_manifest =
        manifest(serde_json::json!({"hud":true,"camera":true,"commands":["help","list"]}));
    let next_manifest = manifest(serde_json::json!({"hud":true,"commands":["help"]}));
    assert!(AppBackend::marketplace_subset(&old_manifest, &next_manifest).unwrap());
    assert!(
        !AppBackend::marketplace_subset(
            &old_manifest,
            &manifest(serde_json::json!({"commands":["op"]}))
        )
        .unwrap()
    );
    assert!(
        AppBackend::marketplace_subset(
            &old_manifest,
            &manifest(serde_json::json!({"future":true}))
        )
        .is_err()
    );
    for enabled in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().canonicalize().unwrap();
        let config = config_root.as_path();
        let old = config.join("old.wasm");
        let next = config.join("next.wasm");
        let before = config.join("before.wasm");
        let after = config.join("after.wasm");
        let saved = mod_host::ModGrants {
            hud: false,
            camera: true,
            commands: vec!["help".into(), "list".into(), "op".into()],
            ..Default::default()
        };
        let entries = vec![
            registration::Entry {
                component: before.clone(),
                ..Default::default()
            },
            registration::Entry {
                component: old.clone(),
                enabled,
                grants: saved,
                ..Default::default()
            },
            registration::Entry {
                component: after.clone(),
                ..Default::default()
            },
        ];
        fs::write(config.join(registration::REGISTRATION_FILE),serde_json::to_vec(&serde_json::json!({"version":registration::VERSION,"request_id":"fixture","mods":entries})).unwrap()).unwrap();
        let previous = selection::registered(config)
            .into_iter()
            .find(|entry| entry.entry.component == old)
            .unwrap();
        AppBackend::replace_marketplace(
            config,
            Some(&previous),
            &next,
            &next_manifest,
            "update".into(),
        )
        .unwrap();
        let entries = selection::registered(config);
        assert_eq!(
            entries
                .iter()
                .map(|entry| &entry.entry.component)
                .collect::<Vec<_>>(),
            vec![&before, &next, &after]
        );
        assert_eq!(entries[1].entry.enabled, enabled);
        assert!(
            !entries[1].entry.grants.hud,
            "a declined grant stays declined"
        );
        assert!(!entries[1].entry.grants.camera);
        assert_eq!(entries[1].entry.grants.commands, vec!["help"]);
    }
}

#[test]
fn failed_marketplace_handoff_resumes_unchanged_enabled_selection() {
    use launcher_host::mod_library::port::Backend;
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old.wasm");
    select(root.path(), &old, true, "enabled".into()).unwrap();
    let before = registered(root.path()).remove(0).entry;
    // A maximum-length request must still allow rollback to publish a fresh acknowledgement ID.
    let receipt = AppBackend::pause_marketplace(
        root.path(),
        &old,
        "a".repeat(registration::MAX_REQUEST_BYTES),
    )
    .unwrap();
    assert_eq!(registered(root.path())[0].entry, before);
    AppBackend::resume_marketplace(root.path(), &receipt).unwrap();
    assert_eq!(registered(root.path())[0].entry, before);
    let record: serde_json::Value = serde_json::from_slice(
        &fs::read(root.path().join(registration::REGISTRATION_FILE)).unwrap(),
    )
    .unwrap();
    assert!(record.get("suspended").is_none());
    assert_ne!(
        record["request_id"].as_str(),
        Some("a".repeat(registration::MAX_REQUEST_BYTES).as_str())
    );
}
