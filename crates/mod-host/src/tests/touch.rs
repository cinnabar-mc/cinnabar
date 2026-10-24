use super::*;

fn source(init: &str, frame: &str, denied: bool) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("touch.wat")
        .replace("$TOUCH", &format!("{name}/touch@{version}"))
        .replace("$ERROR", if denied { "1" } else { "0" })
        .replace("$INIT", init)
        .replace("$FRAME", frame)
}

fn load(granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("touch.wat");
    std::fs::write(&path, source("call $buttons", "call $presses", !granted)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            controls: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

/// Buttons and taps cross the component boundary; the guest sees a tap in its next frame.
#[test]
fn buttons_and_taps_cross_the_component_boundary() {
    let (_directory, mut host) = load(true);
    assert_eq!(
        host.touch_buttons(),
        [ui::mod_touch::TouchButton {
            id: "demo-menu".into(),
            label: "Menu".into(),
            anchor: ui::mod_hud::Anchor::TopRight,
            offset: [4., 24.],
            size: [36., 18.],
        }]
    );
    host.frame(false).unwrap();
    assert_eq!(host.touch_buttons().len(), 1);
    assert!(!host.queue_touch_press("other"));
    assert!(host.queue_touch_press("demo-menu"));
    host.frame(false).unwrap();
    assert!(host.touch_buttons().is_empty());
    assert!(host.is_active());
}

/// Without controls authority both imports refuse and nothing is presented.
#[test]
fn buttons_require_controls_authority() {
    let (_directory, mut host) = load(false);
    host.frame(false).unwrap();
    assert!(host.touch_buttons().is_empty());
    assert!(!host.queue_touch_press("demo-menu"));
    assert!(host.is_active());
}
