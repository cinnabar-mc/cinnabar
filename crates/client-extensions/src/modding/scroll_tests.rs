use super::super::*;

/// Writes a real component importing only scroll observations, reservations and a result label.
fn probe(path: &Path, capture: bool, frame_end: &str) -> ModHost {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let source = include_str!("scroll_probe.wat")
        .replace("$INPUT", &format!("{name}/input@{version}"))
        .replace("$HUD", &format!("{name}/hud@{version}"))
        .replace("$CLAIM_COUNT", if capture { "1" } else { "0" })
        .replace("$FRAME_END", frame_end);
    std::fs::write(path, source).unwrap();
    ModHost::load_with_grants(
        path,
        ModGrants {
            controls: true,
            hud: true,
            ..Default::default()
        },
    )
    .unwrap()
}

fn wheel() -> mod_host::ScrollFrame {
    mod_host::ScrollFrame {
        line_y: 1.0,
        ..mod_host::empty_scroll()
    }
}

/// Runs the actual component callback, which publishes whether it read a nonzero wheel delta.
fn read(host: &mut ModHost) {
    let controls = ControlFrame {
        focused: true,
        ..mod_host::empty_controls()
    };
    host.frame_with_controls(false, None, controls).unwrap();
}

#[test]
fn wheel_capture_uses_load_order_and_releases_when_the_chord_is_not_held() {
    let dir = tempfile::tempdir().unwrap();
    let observer = probe(&dir.path().join("observer.wat"), false, "");
    let first = probe(&dir.path().join("first.wat"), true, "");
    let second = probe(&dir.path().join("second.wat"), true, "");
    let mut hosts = ModHosts::new(observer, vec![first, second]);
    assert!(hosts.route_scroll(&wheel(), &["KeyC".into()]));
    for index in 0..hosts.host_count() {
        read(hosts.host_mut(index));
    }
    assert_eq!(hosts.host(0).label(), Some("scroll"));
    assert_eq!(hosts.host(1).label(), Some("scroll"));
    assert_eq!(hosts.host(2).label(), Some("none"));
    assert!(!hosts.route_scroll(&wheel(), &[]));
    for index in 0..hosts.host_count() {
        read(hosts.host_mut(index));
        assert_eq!(hosts.host(index).label(), Some("scroll"));
    }
    // A later callback without a fresh sample cannot replay consumed observations.
    read(hosts.host_mut(1));
    assert_eq!(hosts.host(1).label(), Some("none"));
}

#[test]
fn removed_and_reloaded_mods_do_not_retain_the_old_reservation() {
    let dir = tempfile::tempdir().unwrap();
    let first_path = dir.path().join("first.wat");
    let first = probe(&first_path, true, "");
    let second = probe(&dir.path().join("second.wat"), true, "");
    let mut hosts = ModHosts::new(first, vec![second]);
    // Reload the first slot with an observer; its old capture must not survive.
    drop(probe(&first_path, false, ""));
    assert!(hosts.host_mut(0).reload_if_changed().unwrap());
    assert!(hosts.host(0).scroll_capture_keys().is_empty());
    assert!(hosts.route_scroll(&wheel(), &["KeyC".into()]));
    read(hosts.host_mut(1));
    assert_eq!(hosts.host(1).label(), Some("scroll"));
    drop(hosts.remove(1));
    assert!(!hosts.route_scroll(&wheel(), &["KeyC".into()]));
}

#[test]
fn a_trapped_capturer_releases_its_reservation_for_the_next_frame() {
    let dir = tempfile::tempdir().unwrap();
    let first = probe(&dir.path().join("trapped.wat"), true, "unreachable");
    let second = probe(&dir.path().join("healthy.wat"), true, "");
    let mut hosts = ModHosts::new(first, vec![second]);
    assert!(hosts.route_scroll(&wheel(), &["KeyC".into()]));
    assert!(
        hosts
            .host_mut(0)
            .frame_with_controls(
                false,
                None,
                ControlFrame {
                    focused: true,
                    ..mod_host::empty_controls()
                }
            )
            .is_err()
    );
    assert!(!hosts.host(0).is_active());
    assert!(hosts.host(0).scroll_capture_keys().is_empty());
    assert!(hosts.route_scroll(&wheel(), &["KeyC".into()]));
    read(hosts.host_mut(1));
    assert_eq!(hosts.host(1).label(), Some("scroll"));
}
