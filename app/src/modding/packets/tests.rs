use super::*;
use client_runtime::blob_cache::ClientBlobCacheOwner;
use mod_host::ModGrants;

#[test]
fn startup_cache_uses_enabled_packet_consent_before_program_admission() {
    let root = tempfile::tempdir().unwrap();
    let owner = ClientBlobCacheOwner::default();
    let mods = PacketMods::default();
    for (enabled, packets, expected) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
    ] {
        let entry = Entry {
            component: root.path().join("packet.wasm"),
            enabled,
            grants: ModGrants {
                packets,
                ..Default::default()
            },
            ..Default::default()
        };
        std::fs::write(
            root.path()
                .join(super::super::registration::REGISTRATION_FILE),
            serde_json::to_vec(&serde_json::json!({
                "version": super::super::registration::VERSION,
                "request_id": "cache-fixture", "mods": [entry]
            }))
            .unwrap(),
        )
        .unwrap();
        let cache = mods.startup_cache(&owner, root.path());
        assert_eq!(cache.enables_upstream_client_cache(), expected);
        assert_eq!(cache.cache().is_some(), expected);
    }
}

#[test]
fn late_packet_selection_waits_for_a_connection_with_inline_chunks() {
    let root = tempfile::tempdir().unwrap();
    let owner = ClientBlobCacheOwner::default();
    let mut mods = PacketMods::default();
    let original = PacketMods::connection_cache(&owner, Some(&mods), []);
    assert!(original.enables_upstream_client_cache());
    mods.select_environment(PacketModSelection {
        path: root.path().join("packet.wasm"),
        grants: ModGrants {
            packets: true,
            ..Default::default()
        },
    });
    assert!(
        mods.session_relay(original.enables_upstream_client_cache())
            .is_none()
    );
    let reconnected = PacketMods::connection_cache(&owner, Some(&mods), []);
    assert!(!reconnected.enables_upstream_client_cache());
    assert!(
        mods.session_relay(reconnected.enables_upstream_client_cache())
            .is_some()
    );
    assert!(
        original.cache().is_some(),
        "the active connection retains its cache"
    );
}

#[test]
fn sessions_without_packet_mods_keep_the_core_cache_enabled() {
    let layout = launcher::test_support::scratch("ordinary-session-cache");
    let owner = ClientBlobCacheOwner::default();
    let cache = PacketMods::connection_cache(&owner, None, [ModGrants::default()]);
    let command = launcher_host::core_process::core_command_for_address(
        &layout,
        Path::new("core"),
        &layout.runtime_root,
        "fixture.invalid",
        None,
        cache.enables_upstream_client_cache(),
    );
    assert!(
        command
            .get_args()
            .any(|arg| arg == "-upstream-client-cache")
    );
    assert!(cache.cache().is_some());
}
