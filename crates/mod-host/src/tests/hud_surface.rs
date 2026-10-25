use super::*;
use std::sync::Arc;
use ui::mod_hud::{ElementState, HudViewport, VanillaElement};

/// A two-control HUD surface whose FPS label reads `#fps`.
const SURFACE: &str = r##"{"screen":"hud.root","document":"{\"namespace\":\"hud\",\"root\":{\"type\":\"panel\",\"controls\":[{\"fps\":{\"type\":\"label\",\"text\":\"#fps\",\"bindings\":[{\"binding_name\":\"#fps\"}]}},{\"helmet\":{\"type\":\"custom\",\"renderer\":\"inventory_item_renderer\",\"size\":[16,16],\"bindings\":[{\"binding_name\":\"#helmet\",\"binding_name_override\":\"#item_renderer_data\"}]}}]}}","bindings":{"#fps":"60","#helmet":{"item":{"identifier":"minecraft:iron_helmet"}}}}"##;
const PATCH: &str = r##"{"binding_changes":{"#fps":"144"}}"##;
const TEXT: &str = "Armor 85%";
const VIEWPORT: HudViewport = HudViewport {
    width: 427.,
    height: 240.,
    scale: 3.,
};

/// Fixed metrics: four GUI units per character in the sans face.
struct Metrics;

impl HudTextMeasure for Metrics {
    fn width(&self, text: &str, font: ui::mod_panel::LabelFont) -> Option<f32> {
        (font == ui::mod_panel::LabelFont::Sans).then_some(text.chars().count() as f32 * 4.)
    }
}

fn escape(text: &str) -> String {
    text.as_bytes()
        .iter()
        .map(|b| format!("\\{b:02x}"))
        .collect()
}

fn source(init: &str, frame: &str, error: bool) -> String {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("hud_surface.wat")
        .replace("$HUD", &format!("{name}/hud@{version}"))
        .replace("$SURFACE_LENGTH", &SURFACE.len().to_string())
        .replace("$PATCH_LENGTH", &PATCH.len().to_string())
        .replace("$TEXT_LENGTH", &TEXT.len().to_string())
        .replace("$SURFACE", &escape(SURFACE))
        .replace("$PATCH", &escape(PATCH))
        .replace("$TEXT", &escape(TEXT))
        .replace("$WIDTH", &VIEWPORT.width.to_string())
        .replace("$SCALE", &VIEWPORT.scale.to_string())
        .replace("$INIT", init)
        .replace("$FRAME", frame)
        .replace("$ERROR", if error { "1" } else { "0" })
}

fn load(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hud_surface.wat");
    std::fs::write(&path, source(init, frame, !granted)).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            hud: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (dir, host)
}

/// The committed surface's `#fps` text.
fn fps(host: &ModHost) -> Option<ui::mod_panel::SurfaceValue> {
    host.hud_surface()
        .and_then(|(surface, _)| surface.bindings.get("#fps").cloned())
}

#[test]
fn hud_surfaces_publish_patch_and_clear_through_the_guest() {
    let (_dir, mut host) = load("call $surface", "call $patch", true);
    let (surface, first) = host.hud_surface().map(|(s, r)| (Arc::clone(s), r)).unwrap();
    assert_eq!(surface.screen, "hud.root");
    assert_eq!(
        fps(&host),
        Some(ui::mod_panel::SurfaceValue::Text("60".into()))
    );
    assert!(surface.bindings["#helmet"].item().is_some());
    host.frame(false).unwrap();
    let (patched, second) = host.hud_surface().map(|(s, r)| (Arc::clone(s), r)).unwrap();
    assert_ne!(first, second, "a committed patch changes the revision");
    assert_eq!(patched.document, surface.document);
    assert_eq!(
        fps(&host),
        Some(ui::mod_panel::SurfaceValue::Text("144".into()))
    );
    assert_eq!(patched.bindings["#helmet"], surface.bindings["#helmet"]);
    let (_dir, mut host) = load("call $surface", "call $clear", true);
    host.frame(false).unwrap();
    assert!(host.hud_surface().is_none());
}

#[test]
fn hud_surfaces_need_hud_authority_and_a_successful_callback() {
    let (_dir, mut host) = load("call $surface call $hide-two", "call $patch", false);
    host.frame(false).unwrap();
    assert!(host.hud_surface().is_none());
    assert!(host.hidden_hud_elements().is_empty());
    assert!(host.is_active());
    let (_dir, mut host) = load("", "call $surface call $hide-two unreachable", true);
    assert!(host.frame(false).is_err());
    assert!(host.hud_surface().is_none());
    assert!(host.hidden_hud_elements().is_empty());
}

#[test]
fn hidden_elements_are_retained_until_replaced() {
    let (_dir, mut host) = load("call $hide-two", "", true);
    assert_eq!(
        host.hidden_hud_elements().iter().collect::<Vec<_>>(),
        [VanillaElement::Crosshair, VanillaElement::Hunger]
    );
    host.frame(false).unwrap();
    assert!(host.hidden_hud_elements().contains(VanillaElement::Hunger));
    let (_dir, mut host) = load("call $hide-two", "call $show-all", true);
    host.frame(false).unwrap();
    assert!(host.hidden_hud_elements().is_empty());
}

#[test]
fn viewport_reads_report_the_last_observed_root() {
    let (_dir, mut host) = load("call $require-no-viewport", "call $require-viewport", true);
    host.observe_hud(Some(VIEWPORT), [ElementState::default(); 4]);
    host.frame(false).unwrap();
    assert!(host.is_active(), "the guest read the observed root");
    let (_dir, mut host) = load("", "call $require-no-viewport", false);
    host.observe_hud(Some(VIEWPORT), [ElementState::default(); 4]);
    host.frame(false).unwrap();
    assert!(host.is_active(), "denied mods read no root");
}

#[test]
fn text_measurement_uses_the_installed_label_metrics() {
    let (_dir, mut host) = load("", "call $measure-sans", true);
    assert!(
        host.frame(false).is_err(),
        "without metrics the measurement errs"
    );
    let (_dir, mut host) = load("", "call $measure-sans", true);
    host.set_text_measure(Some(Arc::new(Metrics)));
    host.frame(false).unwrap();
    assert!(host.is_active());
}
