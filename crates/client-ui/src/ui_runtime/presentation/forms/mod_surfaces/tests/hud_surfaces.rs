//! Mods' HUD surfaces follow the gameplay HUD's visibility, and mods hide vanilla elements.

use super::super::super::{ServerUiPack, snapshot};
use super::mod_hud::{frame, presentation};
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use launcher::menu::settings_options::SettingId;
use mod_ui::{ActorAuthority, PanelUiOwner, PublishedHudSurface};
use std::sync::Arc;
use ui::{
    DpiScale,
    mod_hud::{HiddenElements, VanillaElement},
    mod_panel::Surface,
};

/// A HUD surface with one red rectangle in the top left.
fn surface() -> Arc<Surface> {
    Arc::new(
        serde_json::from_value(serde_json::json!({"screen":"layer.root",
            "document":serde_json::json!({"namespace":"layer","root":{"type":"panel",
                "controls":[{"box":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
                    "size":[40,20],"offset":[10,10],"anchor_from":"top_left","anchor_to":"top_left",
                    "color":[1,0,0,1]}},
                    {"label":{"type":"label","text":"#fps","offset":[10,40],
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "bindings":[{"binding_name":"#fps"}]}}]}}).to_string(),
            "bindings":{"#fps":"60 FPS"}}))
        .unwrap(),
    )
}

/// Publishes `surface` as one mod's layer.
fn publish(presentation: &mut UiPresentationRuntime, surface: Option<&Arc<Surface>>) {
    presentation
        .mod_ui_mut()
        .set_hud_surfaces(surface.into_iter().map(|surface| PublishedHudSurface {
            owner: PanelUiOwner::Environment(0),
            revision: 1,
            surface,
            authority: ActorAuthority::default(),
        }));
}

/// Sets the player's Hide HUD preference.
fn hide_hud(presentation: &mut UiPresentationRuntime, hidden: bool) {
    let mut options = launcher::menu::settings_options::SettingsOptions::default();
    let index = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.id == SettingId::HideHud)
        .unwrap();
    options.set(index, u8::from(hidden).into());
    presentation.set_chat_settings_snapshot((Arc::new(options), None));
}

#[test]
fn hud_surfaces_draw_with_the_gameplay_hud_and_hide_under_f1_and_panels() {
    let player = player_state::PlayerState::new(1);
    let mut p = presentation();
    let base = snapshot::rasterize(&frame(&player, &mut p));
    publish(&mut p, Some(&surface()));
    let drawn = snapshot::rasterize(&frame(&player, &mut p));
    assert!(base != drawn, "the layer draws over gameplay");
    let viewport = p.hud_elements().0.expect("the painted root is reported");
    assert_eq!([viewport.width, viewport.height], [640., 360.]);
    hide_hud(&mut p, true);
    let hidden = snapshot::rasterize(&frame(&player, &mut p));
    hide_hud(&mut p, false);
    assert!(hidden != drawn, "F1 hides the layer with the HUD");
    p.mod_ui_mut()
        .set_mod_panel(
            PanelUiOwner::Environment(0),
            Some(&mod_ui::test_support::sample_panel()),
        )
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    let panel = frame(&player, &mut p);
    p.mod_ui_mut().set_mod_panel_open(false);
    publish(&mut p, None);
    p.mod_ui_mut().set_mod_panel_open(true);
    assert!(
        snapshot::rasterize(&panel) == snapshot::rasterize(&frame(&player, &mut p)),
        "an open panel hides the layer"
    );
    p.mod_ui_mut().set_mod_panel_open(false);
    assert!(base == snapshot::rasterize(&frame(&player, &mut p)));
}

/// A panel that keeps the gameplay HUD keeps mods' layers under it too.
#[test]
fn hud_surfaces_stay_under_panels_that_show_the_gameplay_hud() {
    let player = player_state::PlayerState::new(1);
    let mut p = presentation();
    let mut panel = mod_ui::test_support::sample_panel();
    for shown in [true, false] {
        panel.show_gameplay_hud = shown;
        p.mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
            .unwrap();
        p.mod_ui_mut().set_mod_panel_open(true);
        publish(&mut p, None);
        let without = snapshot::rasterize(&frame(&player, &mut p));
        publish(&mut p, Some(&surface()));
        let with = snapshot::rasterize(&frame(&player, &mut p));
        assert_eq!(
            with != without,
            shown,
            "the layer draws under the panel only while it shows the gameplay HUD"
        );
    }
    hide_hud(&mut p, true);
    panel.show_gameplay_hud = true;
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    let hidden = snapshot::rasterize(&frame(&player, &mut p));
    publish(&mut p, None);
    assert!(
        hidden == snapshot::rasterize(&frame(&player, &mut p)),
        "F1 still hides it"
    );
}

#[test]
fn hidden_crosshair_lays_out_but_draws_nothing_and_stays_reported() {
    use ui_engine::text_metrics::TextMetrics;
    let mut p = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
    )
    .unwrap();
    p.enable_json_ui(crate::test_support::mini_carrier())
        .unwrap();
    p.hud_frame.first_person = true;
    let (namespace, name) = json_ui::vanilla::Reference::HudCrosshairHudCrosshairScreen
        .as_str()
        .split_once('.')
        .unwrap();
    p.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/crosshair_test.json"]}"#.to_vec(),
            ),
            (
                "ui/crosshair_test.json".into(),
                serde_json::to_vec(&serde_json::json!({"namespace":namespace,
                    (name):{"type":"screen","controls":[{"cursor":{"type":"custom",
                        "renderer":"cursor_renderer","size":[16,16]}}]}}))
                .unwrap(),
            ),
        ]],
        ..Default::default()
    });
    let runtime = UiRuntime::new(1);
    let player = player_state::PlayerState::new(1);
    let paint = |p: &mut UiPresentationRuntime| {
        let mut nodes = Vec::new();
        p.append_engine_hud(
            &player,
            &runtime,
            &mut nodes,
            &mut 0,
            TextMetrics::for_viewport([1280, 720], DpiScale::new(1.).unwrap(), None),
            [1280., 720.],
            0,
            true,
        )
        .unwrap();
        nodes.len()
    };
    assert!(paint(&mut p) > 0, "the crosshair fixture renders");
    let shown = p.hud_elements().1[VanillaElement::Crosshair.index()];
    assert!(shown.visible);
    let bounds = shown.bounds.expect("the cursor is laid out");
    assert_eq!([bounds[2], bounds[3]], [16., 16.]);
    p.mod_ui_mut()
        .set_hidden_hud_elements(HiddenElements::from_elements([VanillaElement::Crosshair]));
    assert_eq!(paint(&mut p), 0, "a hidden crosshair draws nothing");
    assert_eq!(
        p.hud_elements().1[VanillaElement::Crosshair.index()],
        shown,
        "vanilla would still draw it, at the same place"
    );
    p.mod_ui_mut()
        .set_hidden_hud_elements(HiddenElements::default());
    assert!(paint(&mut p) > 0);
}

/// A panel that retains gameplay HUD must still clear the skipped crosshair observation.
#[test]
fn skipped_crosshair_scene_clears_visibility_and_recovers_after_panel_close() {
    let mut p = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
    )
    .unwrap();
    p.enable_json_ui_inline(crate::test_support::mini_carrier())
        .unwrap();
    p.hud_frame.first_person = true;
    let references = [
        json_ui::vanilla::Reference::HudHudScreen,
        json_ui::vanilla::Reference::HudCrosshairHudCrosshairScreen,
    ];
    let mut layer = vec![(
        "ui/_ui_defs.json".into(),
        br#"{"ui_defs":["ui/hud_fixture_0.json","ui/hud_fixture_1.json"]}"#.to_vec(),
    )];
    for (index, reference) in references.iter().enumerate() {
        let (namespace, name) = reference.as_str().split_once('.').unwrap();
        layer.push((format!("ui/hud_fixture_{index}.json"), serde_json::to_vec(&serde_json::json!({
            "namespace":namespace,(name):{"type":"screen","absorbs_input":false,
                "is_modal":false,"render_game_behind":true,"render_only_when_topmost":false,"controls":[{"draw":if index == 0 {
                    serde_json::json!({"type":"label","text":"HUD","size":[40,12]})
                } else { serde_json::json!({"type":"custom","renderer":"cursor_renderer","size":[16,16]}) }}]}
        })).unwrap()));
    }
    p.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![layer],
        ..Default::default()
    });
    let player = player_state::PlayerState::new(1);
    frame(&player, &mut p);
    let shown = p.hud_elements().1[VanillaElement::Crosshair.index()];
    assert!(shown.visible && shown.bounds.is_some());
    let mut panel = mod_ui::test_support::sample_panel();
    panel.show_gameplay_hud = true;
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    frame(&player, &mut p);
    let hidden = p.hud_elements().1[VanillaElement::Crosshair.index()];
    assert!(!hidden.visible);
    assert_eq!(
        hidden.bounds, shown.bounds,
        "skipped scenes retain the last measured bounds"
    );
    p.mod_ui_mut().set_mod_panel_open(false);
    frame(&player, &mut p);
    assert_eq!(p.hud_elements().1[VanillaElement::Crosshair.index()], shown);
}

/// The presented HUD preserves cross-subtree paint order and per-tile vector artwork.
#[test]
fn layered_hud_surface_keeps_foreground_text_and_collection_art() {
    use serde_json::json;
    let surface: Surface = serde_json::from_value(json!({"screen":"demo.root",
        "document":json!({"namespace":"demo","root":{"type":"panel","size":["100%","100%"],"controls":[
            {"foreground":{"type":"panel","controls":[{"caption":{"type":"label",
                "text":"HUD foreground","size":[180,30],"offset":[25,30],"layer":10,
                "anchor_from":"top_left","anchor_to":"top_left"}}]}},
            {"background":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
                "size":[180,50],"offset":[20,20],"color":[0.15,0.2,0.3,1],"layer":5,
                "anchor_from":"top_left","anchor_to":"top_left"}},
            {"tiles":{"type":"panel","cinnabar_collection":"tiles","offset":[20,90],
                "anchor_from":"top_left","anchor_to":"top_left"}}
        ]}}).to_string(),
        "collections":{"tiles":{"definition":{"columns":1,"viewport":[200,60],
            "template":{"screen":"tile.root","document":json!({"namespace":"tile","root":{
                "type":"custom","renderer":"cinnabar_vector_path","size":[48,20],
                "color":[1,0.7,0.2,1],"bindings":[
                    {"binding_name":"#item_art_paths","binding_name_override":"#paths"},
                    {"binding_name":"#item_art_view_box","binding_name_override":"#view_box"},
                    {"binding_name":"#item_art_stroke_width","binding_name_override":"#stroke_width"},
                    {"binding_name":"#item_art_closed","binding_name_override":"#closed"}]
            }}).to_string()},"items":[{"art":{"paths":[[[10,10],[90,50],[170,10]]],
                "view_box":[0,0,180,60],"stroke_width":4,"closed":false}}]},"order":[0]}}
    })).unwrap();
    surface.validate_hud().unwrap();
    let player = player_state::PlayerState::new(1);
    let mut p = presentation();
    let without = snapshot::rasterize(&frame(&player, &mut p));
    publish(&mut p, Some(&Arc::new(surface.clone())));
    let drawn = frame(&player, &mut p);
    assert!(
        snapshot::rasterize(&drawn) != without,
        "the mod HUD must draw"
    );
    snapshot::write(&drawn, "mod-hud-surface");
    let mut without_art = surface.clone();
    Arc::make_mut(&mut without_art.collections.get_mut("tiles").unwrap().definition).items[0].art =
        None;
    let mut p = presentation();
    publish(&mut p, Some(&Arc::new(without_art)));
    assert!(
        snapshot::rasterize(&drawn) != snapshot::rasterize(&frame(&player, &mut p)),
        "the collection artwork must paint"
    );
    let mut whole = surface;
    let mut document: serde_json::Value = serde_json::from_str(&whole.document).unwrap();
    document["root"] = json!({"type":"panel","size":["100%","100%"],
        "controls":[{"whole":document["root"].clone()}]});
    whole.document = document.to_string();
    let mut p = presentation();
    publish(&mut p, Some(&Arc::new(whole)));
    assert!(
        snapshot::rasterize(&drawn) == snapshot::rasterize(&frame(&player, &mut p)),
        "the split and whole surface must paint the same pixels"
    );
}
