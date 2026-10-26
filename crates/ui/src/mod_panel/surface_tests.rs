//! Surface-only panels, popup colours and the HUD surface role.

use super::*;

/// Panels carry only a surface and input widgets: removed native layout fields fail to parse.
#[test]
fn panels_require_a_surface_and_reject_removed_native_layout_fields() {
    let wire = serde_json::to_value(super::panel()).unwrap();
    assert!(serde_json::from_value::<Panel>(wire.clone()).is_ok());
    let mut missing = wire.clone();
    missing.as_object_mut().unwrap().remove("surface");
    assert!(serde_json::from_value::<Panel>(missing).is_err());
    for (field, value) in [
        ("title", serde_json::json!("Controls")),
        ("dark", serde_json::json!(true)),
        ("theme", serde_json::json!("monochrome")),
        ("style", serde_json::json!("compact")),
        ("sections", serde_json::json!([])),
    ] {
        let mut removed = wire.clone();
        removed[field] = value;
        assert!(
            serde_json::from_value::<Panel>(removed).is_err(),
            "{field} is still accepted"
        );
    }
    let mut previews = wire;
    previews["surface"]["previews"] = serde_json::json!({});
    assert!(serde_json::from_value::<Panel>(previews).is_err());
}

/// A surface-only panel shaped like a packet mod's menu validates without native fields.
#[test]
fn surface_only_packet_menu_panels_validate() {
    let document = serde_json::json!({"namespace":"lunar","root":{"type":"panel","controls":[
        {"toggle":{"type":"button","size":[20,10],"button_mappings":[{"from_button_id":
            "button.menu_select","to_button_id":"mod.control:0","mapping_type":"pressed"}]}},
        {"knob":{"type":"panel","size":[4,4],"bindings":[{"binding_name":"#row_0_knob_offset",
            "binding_name_override":"#offset"}]}}
    ]}});
    let panel: Panel = serde_json::from_value(serde_json::json!({
        "show_gameplay_hud":true,"toggle_key":"RightShift","motion":true,"capture_key":false,
        "reference_size":[495,304],
        "controls":[{"kind":"toggle","id":"aim","label":"Aim","value":false}],
        "surface":{"screen":"lunar.root","document":document.to_string(),
            "bindings":{"#page":"Combat"},"transitions":{}}
    }))
    .unwrap();
    panel.validate().unwrap();
}

/// Patches recolour popup widgets transactionally without replacing the panel.
#[test]
fn panel_patches_replace_widget_colors() {
    let current = panel();
    let patched = PanelPatch::parse(r#"{"widget_colors":{"accent":[0,1,0,1]}}"#)
        .unwrap()
        .apply(&current, &Default::default())
        .unwrap();
    assert_eq!(
        patched.widget_colors.and_then(|colors| colors.accent),
        Some([0., 1., 0., 1.])
    );
    assert_eq!(patched.surface, current.surface);
    assert!(
        PanelPatch::parse(r#"{"widget_colors":{"accent":[2,0,0,1]}}"#)
            .unwrap()
            .apply(&current, &Default::default())
            .is_err()
    );
    let restored = PanelPatch::parse(r#"{"widget_colors":{}}"#)
        .unwrap()
        .apply(&patched, &Default::default())
        .unwrap();
    assert_eq!(restored.widget_colors, Some(WidgetColors::default()));
}

/// Popup widget colours are optional RGBA channels from 0 to 1.
#[test]
fn widget_colors_are_optional_bounded_channels() {
    let mut panel = panel();
    panel.widget_colors = Some(
        serde_json::from_value(serde_json::json!({
            "background":[0,0,0,0.9],"surface":[0.1,0.1,0.1,1],"text":[1,1,1,1],
            "accent":[1,0.3,0.3,1],"border":[0.2,0.2,0.2,1]
        }))
        .unwrap(),
    );
    panel.validate().unwrap();
    panel.widget_colors = Some(WidgetColors {
        accent: Some([1.5, 0., 0., 1.]),
        ..Default::default()
    });
    assert!(panel.validate().is_err());
    panel.widget_colors = Some(WidgetColors {
        text: Some([f32::NAN, 0., 0., 1.]),
        ..Default::default()
    });
    assert!(panel.validate().is_err());
    assert!(serde_json::from_value::<WidgetColors>(serde_json::json!({"tint":[1,1,1,1]})).is_err());
}

/// Panels retain 256 bindings; HUD surfaces retain 512.
#[test]
fn panel_and_hud_surfaces_have_separate_binding_budgets() {
    let mut surface = surface("mod.close");
    surface.bindings = (0..MAX_SURFACE_BINDINGS)
        .map(|i| (format!("#value_{i}"), SurfaceValue::Number(i as f64)))
        .collect();
    surface.validate_bindings().unwrap();
    surface
        .bindings
        .insert("#excess".into(), SurfaceValue::Bool(true));
    assert!(surface.validate_bindings().is_err());
    surface
        .validate_bindings_as(SurfaceRole::Hud)
        .expect("a HUD surface admits more bindings");
    surface.bindings = (0..=MAX_HUD_SURFACE_BINDINGS)
        .map(|i| (format!("#value_{i}"), SurfaceValue::Number(i as f64)))
        .collect();
    assert!(surface.validate_bindings_as(SurfaceRole::Hud).is_err());
}

/// A HUD document's root and named controls with `children`.
fn hud(root: serde_json::Value, named: serde_json::Value) -> Surface {
    let mut document = serde_json::json!({"namespace":"hud","root":root});
    document
        .as_object_mut()
        .unwrap()
        .extend(named.as_object().unwrap().clone());
    Surface {
        screen: "hud.root".into(),
        document: document.to_string(),
        bindings: Default::default(),
        transitions: Default::default(),
        collections: Default::default(),
    }
}

/// HUD surfaces admit the primitive renderers and paint transforms panels use.
#[test]
fn hud_surfaces_share_the_panel_primitive_renderers() {
    let primitives = serde_json::json!({"type":"panel","controls":[
        {"scaled":{"type":"panel","size":[40,20],"cinnabar_transform":{"scale":1},
            "bindings":[{"binding_name":"#preview_scale","binding_name_override":"#cinnabar_scale"}],
            "controls":[{"fill":{"type":"custom","renderer":json_ui::rounded::RENDERER,
                "size":[40,20],"radius":4,"color":[1,1,1,1]}}]}},
        {"path":{"type":"custom","renderer":json_ui::vector_path::RENDERER,"size":[10,10],
            "paths":[[[0,0],[1,1]]],"view_box":[0,0,1,1],"stroke_width":1,"closed":false}},
        {"item":{"type":"custom","renderer":"inventory_item_renderer","size":[16,16],
            "bindings":[{"binding_name":"#helmet","binding_name_override":"#item_renderer_data"}]}}
    ]});
    let surface = hud(primitives.clone(), serde_json::json!({}));
    surface.validate_hud().unwrap();
    let mut panel = panel();
    panel.controls.clear();
    panel.surface = hud(primitives, serde_json::json!({}));
    panel.validate().unwrap();
}

/// HUD surfaces draw without input, so actions, embedded HUD controls and backdrops are refused.
#[test]
fn hud_surfaces_reject_input_actions_embedding_and_backdrops() {
    let label =
        serde_json::json!({"type":"label","text":"#fps","bindings":[{"binding_name":"#fps"}]});
    let plain = hud(
        serde_json::json!({"type":"panel","controls":[{"fps":label}]}),
        serde_json::json!({"armor":{"type":"panel","size":[10,10]}}),
    );
    plain.validate_hud().unwrap();
    let button = hud(
        serde_json::json!({"type":"panel","controls":[{"press":{"type":"button",
            "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.close",
                "mapping_type":"pressed"}]}}]}),
        serde_json::json!({}),
    );
    assert!(button.validate_hud().is_err());
    let embedded =
        serde_json::json!({"type":"custom","renderer":hud_control::RENDERER,"control":"armor"});
    let nested = hud(
        serde_json::json!({"type":"panel","controls":[{"card":embedded.clone()}]}),
        serde_json::json!({"armor":{"type":"panel"}}),
    );
    assert!(nested.validate_hud().is_err());
    let glass = hud(
        serde_json::json!({"type":"panel","controls":[{"pane":{"type":"custom",
            "renderer":backdrop::GLASS_RENDERER,"size":[10,10]}}]}),
        serde_json::json!({}),
    );
    assert!(glass.validate_hud().is_err());
    let mut panel = panel();
    panel.surface = hud(
        serde_json::json!({"type":"panel","controls":[{"card":embedded}]}),
        serde_json::json!({}),
    );
    panel.controls.clear();
    panel.validate().expect("panels embed HUD controls");
}

/// Named HUD controls embed as their own roots, keeping only the collections placed in them.
#[test]
fn named_hud_controls_embed_as_validated_roots() {
    let surface = hud(
        serde_json::json!({"type":"panel","controls":[{"armor@hud.armor":{}}]}),
        serde_json::json!({"armor":{"type":"panel","size":[40,10],"controls":[{"icon":{
            "type":"custom","renderer":"inventory_item_renderer","size":[8,8],
            "bindings":[{"binding_name":"#helmet","binding_name_override":"#item_renderer_data"}]}}]},
            "broken":{"type":"factory_panel"}}),
    );
    let embedded = surface.embedded("armor").unwrap();
    assert_eq!(embedded.screen, "hud.armor");
    assert!(
        embedded
            .resolved_document()
            .unwrap()
            .contains("inventory_item_renderer")
    );
    assert!(surface.embedded("missing").is_err());
    assert!(surface.embedded("broken").is_err());
    assert!(surface.embedded("other.armor").is_err());
}

/// Item and projected values are ordinary bindings of panel and HUD surfaces.
#[test]
fn item_and_projected_values_bind_in_panels_and_huds() {
    let mut surface = surface("mod.close");
    surface.bindings.insert(
        "#helmet".into(),
        serde_json::from_value(serde_json::json!({"item":{"identifier":"minecraft:iron_helmet"}}))
            .unwrap(),
    );
    surface.bindings.insert(
        "#target".into(),
        serde_json::from_value(serde_json::json!({"project":{"actor":9,"offset":[0,2,0]}}))
            .unwrap(),
    );
    surface.validate_bindings().unwrap();
    surface.validate_bindings_as(SurfaceRole::Hud).unwrap();
    surface
        .bindings
        .insert("#target_visible".into(), SurfaceValue::Bool(true));
    assert!(
        surface.validate_bindings().is_err(),
        "companion is host-owned"
    );
}
