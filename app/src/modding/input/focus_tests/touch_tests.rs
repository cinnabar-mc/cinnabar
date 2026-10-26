//! Touch ownership across pointer-driven panel closure and gameplay sampling.

use super::*;
use crate::ui_runtime::{
    gameplay_touch::drive_gameplay_touch_targets, touch_pointer::emulate_ui_pointer,
};
use bevy::input::touch::{TouchInput, TouchPhase};

/// Places the panel's Close control over a mod button and uses production input ordering.
fn touch_panel_app() -> (App, Entity, Vec2) {
    let mut surface = mod_ui::test_support::sample_surface(&[]);
    let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
    let close = &mut document["root"]["controls"][0]["close"];
    close["anchor_from"] = "center".into();
    close["anchor_to"] = "center".into();
    close["offset"] = serde_json::json!([0, 0]);
    surface.document = document.to_string();
    let component = fixture_with_panel(
        &serde_json::json!({
            "toggle_key": "F10", "controls": [], "surface": surface,
        })
        .to_string(),
    );
    let (mut app, window) = personal_panel_app_with_fixture(&component);
    app.add_plugins(bevy::input::InputPlugin)
        .init_resource::<crate::ui_runtime::touch_pointer::UiTouchStarts>()
        .init_resource::<crate::semantic_controls::SemanticTouchTargets>()
        .add_systems(
            PreUpdate,
            emulate_ui_pointer.before(bevy::input::InputSystems),
        )
        .add_systems(
            Update,
            drive_gameplay_touch_targets.after(prepare_mod_input),
        );
    let button = ui::mod_touch::TouchButton {
        id: "menu".into(),
        label: "Menu".into(),
        anchor: ui::mod_hud::Anchor::Center,
        offset: [0.; 2],
        size: [48.; 2],
    };
    let player = app
        .world()
        .resource::<client_state::PlayerRuntime>()
        .clone();
    let ui = app.world().resource::<UiRuntimeResource>().clone();
    let mut presentation = app.world_mut().resource_mut::<UiPresentationResource>();
    // A minimal HUD scene paints the touch overlay without installed game assets.
    let carrier = client_ui::test_support::mini_carrier();
    let mut files = carrier.ui_files().to_vec();
    let definitions = files
        .iter_mut()
        .find(|file| file.path.as_ref() == "ui/_ui_defs.json")
        .unwrap();
    definitions.bytes = br#"{"ui_defs":["ui/server_form.json","ui/touch_test_hud.json"]}"#
        .to_vec()
        .into();
    files.push(assets::UiFile {
        path: "ui/touch_test_hud.json".into(),
        bytes: br#"{"namespace":"hud","hud_screen":{"type":"screen","absorbs_input":false,"is_modal":false,"render_game_behind":true}}"#.to_vec().into(),
    });
    let bytes = assets::encode_ui_catalog(
        carrier.source_manifest_sha256(),
        carrier.atlas_pages(),
        &[],
        &[],
        &files,
    )
    .unwrap();
    presentation
        .enable_json_ui_inline(std::sync::Arc::new(
            assets::RuntimeUiAssets::decode(&bytes).unwrap(),
        ))
        .unwrap();
    presentation.enable_touch_controls(true);
    presentation.set_mod_touch_buttons([(1, &button)].into_iter());
    render(&mut presentation, &player, &ui);
    assert_eq!(
        presentation
            .gameplay_touch_region([0.5; 2], [1280., 720.])
            .unwrap()
            .hit_id,
        semantic_input::touch::mod_button(0).unwrap(),
    );
    (app, window, Vec2::new(640., 360.))
}

/// Sends a physical touch without bypassing pointer emulation or gameplay's message reader.
fn touch(app: &mut App, window: Entity, id: u64, phase: TouchPhase, position: Vec2) {
    app.world_mut().write_message(TouchInput {
        window,
        id,
        phase,
        position,
        force: None,
    });
}

/// Closing a panel cannot also tap the button beneath it, even for batched or extra fingers.
#[test]
fn panel_close_touch_never_becomes_a_mod_button_tap() {
    for batched_release in [false, true] {
        let (mut app, window, at) = touch_panel_app();
        for id in [1, 2] {
            touch(&mut app, window, id, TouchPhase::Started, at);
            if batched_release {
                touch(&mut app, window, id, TouchPhase::Ended, at);
            }
        }
        app.update();
        assert!(
            !app.world()
                .resource::<UiPresentationResource>()
                .mod_ui()
                .mod_panel_open()
        );
        assert!(
            !app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .panel_open()
        );
        if !batched_release {
            for id in [1, 2] {
                touch(&mut app, window, id, TouchPhase::Ended, at);
            }
            app.update();
        }
        assert!(
            app.world_mut()
                .resource_mut::<UiPresentationResource>()
                .take_mod_touch_taps()
                .is_empty()
        );

        // The OS may reuse a released id; that independent gesture must still work.
        touch(&mut app, window, 1, TouchPhase::Started, at);
        app.update();
        touch(&mut app, window, 1, TouchPhase::Ended, at);
        app.update();
        assert_eq!(
            app.world_mut()
                .resource_mut::<UiPresentationResource>()
                .take_mod_touch_taps(),
            [(1, "menu".to_owned())],
        );
    }
}
