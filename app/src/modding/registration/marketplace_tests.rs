//! Marketplace authority stays within signed releases and is not regained by pending results.

use super::*;
use mod_host::ModGrants;
use sha2::{Digest, Sha256};

/// Signs a fixture release for exactly `grants` and authorizes it at `path` until time 10 000.
fn signed(path: &std::path::Path, grants: &str) -> launcher_host::marketplace::Guard {
    let bytes = crate::modding::marketplace::component_fixture(grants);
    fs::write(path, &bytes).unwrap();
    let manifest = launcher_host::marketplace::Manifest {
        slug: "fixture-mod".into(),
        version: "1.0.0".into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes: bytes.len() as u64,
        grants: serde_json::from_str(grants).unwrap(),
        approved_at: 1,
        client_revision: "a".repeat(40),
        api_revision: None,
        policy: "fixture".into(),
        publication: "manual".into(),
    };
    launcher_host::marketplace::Guard::authorized(path.into(), "11", manifest, 10_000)
}

/// Every capability, plus a command name no fixture release signs.
fn everything() -> ModGrants {
    let mut grants = ModGrants::default();
    for capability in ModGrants::CAPABILITIES {
        (capability.set)(&mut grants, true, None);
    }
    grants.commands = vec!["help".into(), "op".into()];
    grants
}

/// Captures one entry the way the registration worker does, returning its admitted grants.
fn capture(
    entry: &mut Entry,
    guard: &launcher_host::marketplace::Guard,
    now: u64,
) -> Result<ModGrants, String> {
    worker::capture(
        &mut source::Observer::default(),
        entry,
        guard,
        &BTreeMap::new(),
        std::time::Instant::now(),
        now,
    )
    .map(|snapshot| snapshot.registration.grants)
}

#[test]
fn managed_admission_never_exceeds_the_signed_grants() {
    let hud_and_help = ModGrants {
        hud: true,
        commands: vec!["help".into()],
        ..Default::default()
    };
    for (grants, expected) in [
        ("{}", ModGrants::default()),
        (r#"{"hud":true,"commands":["help"]}"#, hud_and_help),
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(format!(
            "{}11-fixture.wasm",
            launcher::mods::MARKETPLACE_FILE_PREFIX
        ));
        let guard = signed(&path, grants);
        let mut entry = Entry {
            component: path.clone(),
            enabled: true,
            grants: everything(),
            ..Default::default()
        };
        assert_eq!(capture(&mut entry, &guard, 5_000), Ok(expected.clone()));
        assert_eq!(entry.grants, expected, "the published selection is capped");

        // Turning a signed capability off is kept.
        entry.grants = ModGrants::default();
        assert_eq!(capture(&mut entry, &guard, 5_000), Ok(ModGrants::default()));

        // Without current signed evidence the entry keeps no authority and is held.
        entry.grants = everything();
        assert!(capture(&mut entry, &guard, 20_000).is_err());
        assert_eq!(entry.grants, ModGrants::default());

        // A local component keeps its explicit grants.
        let local = root.path().join("local.wasm");
        fs::copy(&path, &local).unwrap();
        let mut entry = Entry {
            component: local,
            enabled: true,
            grants: everything(),
            ..Default::default()
        };
        assert_eq!(capture(&mut entry, &guard, 5_000), Ok(everything()));
    }
}

#[test]
fn pending_reload_cannot_reauthorize_a_held_marketplace_packet_program() {
    for prepared_reload in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut world, _messages, _retiring) = world();
        let entry = Entry {
            component: root.path().join(format!(
                "{}packet.wasm",
                launcher::mods::MARKETPLACE_FILE_PREFIX
            )),
            enabled: true,
            grants: mod_host::ModGrants {
                packets: true,
                ..Default::default()
            },
            ..Default::default()
        };
        fs::write(&entry.component, packet_bytes()).unwrap();
        let candidate = Box::new(build_candidate(source_snapshot(entry.clone()).unwrap()).unwrap());
        let CandidateRuntime::Packet(program) = &candidate.runtime else {
            panic!("fixture must compile as a packet program");
        };
        // Seed a previously admitted program; its marketplace evidence is now unavailable.
        crate::modding::packets::publish(
            &mut world,
            entry.clone(),
            candidate.identity,
            program.clone(),
        );
        let relay = {
            let mut packets = world.resource_mut::<crate::modding::packets::PacketMods>();
            assert!(packets.authorize(std::slice::from_ref(&entry)).is_empty());
            packets.session_relay(false).unwrap()
        };
        let identity = protocol::session_wire::SessionIdentity {
            display_name: "Steve".into(),
            xuid: "1".into(),
            uuid: "u".into(),
        };
        assert_eq!(relay.start(&identity, "{}", &[1]).len(), 2);
        assert_eq!(relay.outbound(&[1]), Some(vec![0]));
        world
            .resource::<Watcher>()
            .desired
            .lock()
            .unwrap()
            .registration = Some(Registration {
            suspended: None,
            version: VERSION,
            request_id: "held".into(),
            mods: vec![entry.clone()],
        });
        install::sync_authority(&mut world);
        assert_eq!(relay.outbound(&[1]), None);

        let result = if prepared_reload {
            Ok(candidate)
        } else {
            Err("Source held while reloading.".into())
        };
        apply(
            &mut world,
            vec![entry.clone()],
            vec![Change {
                entry: entry.clone(),
                result,
            }],
        );
        let packets = world.resource::<crate::modding::packets::PacketMods>();
        assert!(!packets.admitted(&entry.component));
        assert!(packets.session_relay(false).is_none());
        assert_eq!(relay.outbound(&[1]), None);
    }
}
