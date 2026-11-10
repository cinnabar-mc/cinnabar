use cinnabar::args::{ClientArgs, ParseOutcome};
use std::{ffi::OsString, fs, path::PathBuf, process::Command};

const FIXTURE_DIRECTORY: &str = "CINNABAR_STARTUP_ORDER_FIXTURE";
const DIRECT_CONNECTION: &str = "CINNABAR_STARTUP_ORDER_DIRECT";

/// Exercises library startup in an isolated process without desktop entry-point dialogs.
#[test]
fn invalid_local_assets_startup_child() {
    let Some(directory) = std::env::var_os(FIXTURE_DIRECTORY) else {
        return;
    };
    let directory = PathBuf::from(directory);
    let mut arguments = vec![
        OsString::from("cinnabar"),
        OsString::from("--assets"),
        directory.join("invalid.mcbea").into_os_string(),
    ];
    if std::env::var(DIRECT_CONNECTION).as_deref() == Ok("true") {
        arguments.extend([
            OsString::from("--socket-dir"),
            directory.join("no-bridge").into_os_string(),
            OsString::from("--address"),
            OsString::from("127.0.0.1:1"),
        ]);
    }
    let ParseOutcome::Run(args) = ClientArgs::parse_from(arguments).unwrap() else {
        panic!("startup fixture requires client arguments");
    };
    let error = cinnabar::run(*args).unwrap_err();
    diagnostics::log_stderr!("{error:#}");
    assert!(diagnostics::console::flush_before_exit());
}

#[test]
fn invalid_local_assets_fail_before_connection_startup() {
    let directory = tempfile::tempdir().unwrap();
    let assets = directory.path().join("invalid.mcbea");
    fs::write(&assets, b"invalid world carrier").unwrap();

    for direct in [false, true] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "startup_order::invalid_local_assets_startup_child",
                "--nocapture",
            ])
            .env(FIXTURE_DIRECTORY, directory.path())
            .env(DIRECT_CONNECTION, direct.to_string())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "startup fixture (direct={direct}) failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        if direct {
            assert!(stderr.contains("load startup world carriers"), "{stderr}");
            assert!(stderr.contains(&assets.display().to_string()), "{stderr}");
            assert!(stderr.contains("truncated world asset blob"), "{stderr}");
        } else {
            assert!(stderr.contains("load required menu assets"), "{stderr}");
            assert!(stderr.contains("make atmosphere-assets"), "{stderr}");
        }
        assert!(!stderr.contains("spawn Go core"), "{stderr}");
        assert!(!stderr.contains("wait for Go core"), "{stderr}");
    }
}

#[test]
fn invalid_required_entity_artwork_fails_before_the_menu_or_connection() {
    use assets::carriers::startup::{
        atmosphere_asset_path, entity_asset_path, test_carriers::synthetic_atmosphere_blob,
    };
    let directory = tempfile::tempdir().unwrap();
    let world = directory.path().join("invalid.mcbea");
    fs::write(atmosphere_asset_path(&world), synthetic_atmosphere_blob()).unwrap();
    let entity = entity_asset_path(&world);
    fs::write(&entity, b"invalid entity carrier").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "startup_order::invalid_local_assets_startup_child",
            "--nocapture",
        ])
        .env(FIXTURE_DIRECTORY, directory.path())
        .env(DIRECT_CONNECTION, "false")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("load required menu assets"), "{stderr}");
    assert!(stderr.contains(&entity.display().to_string()), "{stderr}");
    assert!(stderr.contains("make entity-assets"), "{stderr}");
    assert!(!stderr.contains("first frame"), "{stderr}");
    assert!(!stderr.contains("wait for Go core"), "{stderr}");
}
