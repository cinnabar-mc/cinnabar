use super::*;

#[test]
fn grant_all_enables_capabilities_and_only_declared_command_names() {
    for declared in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mods = directory.path().join("mods");
        let registration = directory.path().join("config");
        fs::create_dir_all(&mods).unwrap();
        let component = mods.join("mod.wasm");
        let requested = mod_host::ModGrants {
            commands: vec!["ability".into(), "say".into()],
            ..Default::default()
        };
        if declared {
            requested_component(&component, &requested);
        } else {
            fs::write(&component, wat::parse_str("(component)").unwrap()).unwrap();
        }
        let other = mods.join("other.wasm");
        fs::write(&other, wat::parse_str("(component)").unwrap()).unwrap();
        select(&registration, &component, true, "enable".into()).unwrap();
        select(&registration, &other, true, "other".into()).unwrap();
        let before = registered(&registration);
        let library = scan(&mods, &registration, String::new());
        let id = library
            .installed
            .iter()
            .find(|item| item.file == "mod.wasm")
            .unwrap()
            .id;
        let mut requests = 0;
        for action in [Action::GrantAll(id), Action::RevokeAll(id)] {
            perform::<AppBackend>(
                action,
                &mods,
                &registration,
                &mut requests,
                &mut || Ok(None),
                &mut |_| Ok(retirement_permit()),
            )
            .unwrap();
            let after = registered(&registration);
            assert!(after[0].entry.enabled);
            assert_eq!(after[1].entry, before[1].entry);
            if matches!(action, Action::GrantAll(_)) {
                let grants = &after[0].entry.grants;
                grants.validate().unwrap();
                assert_eq!(
                    grants.commands,
                    if declared {
                        requested.commands.clone()
                    } else {
                        Vec::new()
                    }
                );
                for capability in mod_host::ModGrants::CAPABILITIES {
                    assert_eq!(
                        (capability.enabled)(grants),
                        match capability.name {
                            "commands" => declared,
                            "packets" => false,
                            _ => true,
                        },
                        "{}",
                        capability.name
                    );
                }
            } else {
                assert_eq!(after[0].entry.grants, mod_host::ModGrants::default());
            }
        }
        assert_eq!(requests, 2);
    }
}

#[test]
fn grant_all_rejects_unreadable_or_malformed_declarations_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let component = directory.path().join("mod.wasm");
    let registration = directory.path().join("config");
    select(&registration, &component, true, "enable".into()).unwrap();
    let record = registration.join(registration::REGISTRATION_FILE);
    let before = fs::read(&record).unwrap();
    assert!(selection::grant_all(&registration, &component, "missing".into()).is_err());
    fs::write(&component, b"\0asm").unwrap();
    assert!(selection::grant_all(&registration, &component, "malformed".into()).is_err());
    requested_component(
        &component,
        &mod_host::ModGrants {
            commands: vec!["Invalid command".into()],
            ..Default::default()
        },
    );
    assert!(selection::grant_all(&registration, &component, "invalid".into()).is_err());
    assert_eq!(fs::read(&record).unwrap(), before);
}

#[test]
fn batch_permission_actions_grant_exact_requests_and_revoke_all() {
    let directory = tempfile::tempdir().unwrap();
    let (mods, registration) = (
        directory.path().join("mods"),
        directory.path().join("config"),
    );
    fs::create_dir_all(&mods).unwrap();
    let component = mods.join("zoom.wasm");
    let requested = mod_host::ModGrants {
        camera: true,
        settings: true,
        commands: vec!["ability".into()],
        ..Default::default()
    };
    requested_component(&component, &requested);
    let other = mods.join("other.wasm");
    fs::write(&other, b"text fixture").unwrap();
    select(&registration, &component, true, "enable".into()).unwrap();
    select(&registration, &other, true, "other".into()).unwrap();
    selection::permission(&registration, &component, 0, true, "extra".into()).unwrap();
    let before = registered(&registration);
    let shown = scan(&mods, &registration, String::new());
    let id = shown
        .installed
        .iter()
        .find(|item| item.file == "zoom.wasm")
        .unwrap()
        .id;
    let mut requests = 0;
    perform::<AppBackend>(
        Action::AllowRequested(id),
        &mods,
        &registration,
        &mut requests,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    let allowed = registered(&registration);
    assert_eq!(allowed[0].entry.grants, requested);
    assert_eq!(allowed[1].entry, before[1].entry);
    assert_eq!(requests, 1);
    let scanned = scan(&mods, &registration, String::new());
    let item = scanned.installed.iter().find(|item| item.id == id).unwrap();
    for (permission, capability) in item
        .permissions
        .iter()
        .zip(mod_host::ModGrants::CAPABILITIES)
    {
        assert_eq!(permission.name, capability.label);
        assert_eq!(permission.sensitive, capability.sensitive);
        assert_eq!(permission.requested, Some((capability.enabled)(&requested)));
    }
    let updated = mod_host::ModGrants {
        commands: vec!["ability".into(), "say".into()],
        ..requested.clone()
    };
    requested_component(&component, &updated);
    let updated_card = scan(&mods, &registration, String::new());
    let updated_card = updated_card
        .installed
        .iter()
        .find(|item| item.file == "zoom.wasm")
        .unwrap();
    assert_eq!(updated_card.requested_counts(), (2, 3));
    assert!(
        updated_card
            .permissions
            .iter()
            .find(|permission| permission.name == "Commands")
            .unwrap()
            .enabled
    );
    fs::remove_file(&component).unwrap();
    let missing = scan(&mods, &registration, String::new()).active[0].id;
    perform::<AppBackend>(
        Action::RevokeAll(missing),
        &mods,
        &registration,
        &mut requests,
        &mut || Ok(None),
        &mut |_| Ok(retirement_permit()),
    )
    .unwrap();
    let revoked = registered(&registration);
    assert_eq!(revoked[0].entry.grants, mod_host::ModGrants::default());
    assert!(revoked[0].entry.enabled);
    assert_eq!(revoked[1].entry, before[1].entry);
    assert_eq!(requests, 2);
}

#[test]
fn undeclared_and_malformed_requests_cannot_get_batch_grants() {
    let directory = tempfile::tempdir().unwrap();
    let (mods, registration) = (
        directory.path().join("mods"),
        directory.path().join("config"),
    );
    fs::create_dir_all(&mods).unwrap();
    let component = mods.join("mod.wasm");
    select(&registration, &component, true, "enable".into()).unwrap();
    let record = registration.join(registration::REGISTRATION_FILE);
    let before = fs::read(&record).unwrap();
    for bytes in [b"undeclared".as_slice(), b"\0asm".as_slice()] {
        fs::write(&component, bytes).unwrap();
        let id = scan(&mods, &registration, String::new()).active[0].id;
        assert!(
            perform::<AppBackend>(
                Action::AllowRequested(id),
                &mods,
                &registration,
                &mut 0,
                &mut || Ok(None),
                &mut |_| Ok(retirement_permit())
            )
            .is_err()
        );
        assert_eq!(fs::read(&record).unwrap(), before);
    }
}
