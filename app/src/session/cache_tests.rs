use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn session_core_spawn_uses_the_cache_mode_selected_from_active_packet_grants() {
    for packets in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let layout = launcher::test_support::scratch_at(
            launcher::install_layout::Platform::Linux,
            root.path(),
        );
        std::fs::create_dir_all(layout.core_executable.parent().unwrap()).unwrap();
        std::fs::write(
            &layout.core_executable,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nread -r hold\n",
        )
        .unwrap();
        std::fs::set_permissions(
            &layout.core_executable,
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let args_path = PathBuf::from(format!("{}.args", layout.core_executable.display()));
        let menu = MenuRuntime::new_with_layout(
            false,
            None,
            "Fixture".into(),
            layout,
            client_shell::player_skin::LocalPlayerSkin::generated_default("Fixture"),
        );
        let owner = BlobCache::default();
        let cache = crate::modding::packets::PacketMods::connection_cache(
            &owner,
            None,
            [mod_host::ModGrants {
                packets,
                ..Default::default()
            }],
        );
        let mut controller = SessionController::default().with_blob_cache(cache);
        let directory = controller
            .start_core(&menu, "fixture.invalid", None, 1)
            .unwrap();
        test_time::eventually("the session core records its login flags", || {
            args_path.is_file()
        });
        controller.core.stop();
        let args = std::fs::read_to_string(args_path).unwrap();
        assert_eq!(
            args.lines().any(|arg| arg == "-upstream-client-cache"),
            !packets
        );
        assert_eq!(controller.blob_cache.cache().is_some(), !packets);
        drop(directory);
    }
}
