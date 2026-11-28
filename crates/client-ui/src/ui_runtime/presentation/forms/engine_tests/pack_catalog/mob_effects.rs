use json_ui::{Catalog, Draw};
use ui_screens::hud_renderers::{COLUMN_TOP_PROP, JAVA_MOB_EFFECTS_RENDERER, MOB_EFFECTS_RENDERER};

use super::{HudModel, layer, layer_pack_catalog, render_model, vanilla};

/// The effect renderers the HUD screen places while an effect is active, in tree order.
fn effect_renderers(catalog: &Catalog) -> Vec<String> {
    let model = HudModel {
        effects_visible: true,
        ..Default::default()
    };
    render_model(catalog, &model, [480.0, 270.0])
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Custom { renderer, .. }
                if [MOB_EFFECTS_RENDERER, JAVA_MOB_EFFECTS_RENDERER]
                    .contains(&renderer.as_str()) =>
            {
                Some(renderer.clone())
            }
            _ => None,
        })
        .collect()
}

/// A server HUD pack that adds a widget at the top of the screen and never mentions the
/// status effects, as a server's own HUD does.
const TOP_BAR: &[u8] = br#"{
    "namespace":"hud",
    "root_panel":{"modifications":[{"array_name":"controls","operation":"insert_front","value":[
        {"top_bar":{"type":"label","text":"Top bar","size":[200,9],
            "anchor_from":"top_middle","anchor_to":"top_middle"}}
    ]}]}
}"#;

/// Without a server pack the built-in Java pack's corner rows draw.
#[test]
fn builtin_hud_draws_the_java_effect_rows() {
    let Some(vanilla) = vanilla("builtin_hud_draws_the_java_effect_rows") else {
        return;
    };
    assert_eq!(
        effect_renderers(&layer_pack_catalog(&vanilla, &[])),
        [JAVA_MOB_EFFECTS_RENDERER]
    );
}

/// A server pack that authors HUD controls laid its widgets out around vanilla's right-edge
/// column, which sits below the top band, so the Java rows in the top-right corner must not
/// cover them.
#[test]
fn server_hud_pack_gets_vanillas_effect_column() {
    let Some(vanilla) = vanilla("server_hud_pack_gets_vanillas_effect_column") else {
        return;
    };
    let catalog = layer_pack_catalog(&vanilla, &[layer(TOP_BAR)]);
    assert_eq!(effect_renderers(&catalog), [MOB_EFFECTS_RENDERER]);
}

/// The restored column starts under the top bar, not below vanilla's band over the status rows.
#[test]
fn server_hud_column_starts_under_the_top_edge() {
    let Some(vanilla) = vanilla("server_hud_column_starts_under_the_top_edge") else {
        return;
    };
    let catalog = layer_pack_catalog(&vanilla, &[layer(TOP_BAR)]);
    let model = HudModel {
        effects_visible: true,
        ..Default::default()
    };
    let nodes = render_model(&catalog, &model, [480.0, 270.0]).nodes;
    let top = nodes
        .iter()
        .find_map(|node| match &node.draw {
            Draw::Custom { renderer, data } if renderer == MOB_EFFECTS_RENDERER => data
                .get(COLUMN_TOP_PROP)
                .and_then(serde_json::Value::as_f64),
            _ => None,
        })
        .expect("column top");
    assert!(top > 0.0 && top < 40.0, "column top {top}");
}

/// The column is restored whichever layer first authors the HUD, and a later layer still
/// patches it.
#[test]
fn a_lower_layer_without_hud_controls_does_not_hold_back_the_column() {
    let Some(vanilla) = vanilla("a_lower_layer_without_hud_controls_does_not_hold_back_the_column")
    else {
        return;
    };
    let unrelated = vec![(
        "ui/chat_screen.json".to_owned(),
        br#"{"namespace":"chat"}"#.to_vec(),
    )];
    let catalog = layer_pack_catalog(&vanilla, &[unrelated.clone(), layer(TOP_BAR)]);
    assert_eq!(effect_renderers(&catalog), [MOB_EFFECTS_RENDERER]);
    let catalog = layer_pack_catalog(&vanilla, &[layer(TOP_BAR), unrelated]);
    assert_eq!(effect_renderers(&catalog), [MOB_EFFECTS_RENDERER]);
}

/// A pack that only restyles another screen leaves the HUD's Java rows alone.
#[test]
fn server_pack_without_hud_controls_keeps_the_java_effect_rows() {
    let Some(vanilla) = vanilla("server_pack_without_hud_controls_keeps_the_java_effect_rows")
    else {
        return;
    };
    let chat = vec![(
        "ui/chat_screen.json".to_owned(),
        br#"{"namespace":"chat"}"#.to_vec(),
    )];
    assert_eq!(
        effect_renderers(&layer_pack_catalog(&vanilla, &[chat])),
        [JAVA_MOB_EFFECTS_RENDERER]
    );
}

/// A server pack that places its own effects control keeps its geometry on top of vanilla's.
#[test]
fn server_effect_geometry_patches_vanillas_control() {
    let Some(vanilla) = vanilla("server_effect_geometry_patches_vanillas_control") else {
        return;
    };
    let pack = layer(br#"{"namespace":"hud","root_panel/mob_effects_renderer":{"offset":[0,60]}}"#);
    let catalog = layer_pack_catalog(&vanilla, &[pack]);
    let control = catalog
        .lookup("hud", "root_panel")
        .and_then(|root| {
            root.children
                .iter()
                .find(|child| child.name == "mob_effects_renderer")
        })
        .expect("effects control");
    assert_eq!(control.props["offset"], serde_json::json!([0, 60]));
    assert_eq!(effect_renderers(&catalog), [MOB_EFFECTS_RENDERER]);
}
