//! Replaced control definitions rebuild only the parts and embedded controls that read them,
//! and embedded controls make way for newer ones least recently drawn first.

use std::sync::Arc;

use serde_json::{Value, json};
use ui::mod_panel::{Surface, SurfacePatch, SurfaceRole, SurfaceValue};

use super::layer::{Layer, MAX_EMBEDDED};
use super::tests::Harness;
use super::{ActorAuthority, PublishedHudSurface};
use crate::{ModUi, PanelUiOwner};

const OWNER: PanelUiOwner = PanelUiOwner::Environment(0);

/// A `width` by 10 rectangle at `offset` whose alpha reads `#shade`.
fn card(width: u32, offset: [u32; 2]) -> Value {
    json!({"type":"panel","size":[width,10],"offset":offset,"anchor_from":"top_left",
        "anchor_to":"top_left","controls":[{"fill":{"type":"custom",
            "renderer":"cinnabar_rounded_rectangle","size":["100%","100%"],"color":[1,0,0,1],
            "bindings":[{"binding_name":"#shade","binding_name_override":"#alpha"}]}}]})
}

/// Three root cards, two through definitions, and two preview-only controls, one of which
/// instances `card_a`.
fn surface() -> Arc<Surface> {
    Arc::new(
        serde_json::from_value(json!({"screen":"hud.root",
            "document":json!({"namespace":"hud",
                "root":{"type":"panel","size":["100%","100%"],"controls":[
                    {"a@hud.card_a":{}},{"b@hud.card_b":{}},{"c":card(20,[10,70])}]},
                "card_a":card(20,[10,10]),
                "card_b":card(20,[10,40]),
                "preview_a":{"type":"panel","size":[40,20],"controls":[{"inner@hud.card_a":{}}]},
                "preview_c":card(30,[0,0])
            }).to_string(),
            "bindings":{"#shade":1}}))
        .unwrap(),
    )
}

/// `surface` with `patch` applied as `hud.update-surface` applies it.
fn patched(surface: &Surface, patch: Value) -> Arc<Surface> {
    Arc::new(
        SurfacePatch::parse(&patch.to_string())
            .unwrap()
            .apply(surface, SurfaceRole::Hud)
            .unwrap(),
    )
}

fn publish(ui: &mut ModUi, revision: u64, surface: &Arc<Surface>) {
    ui.set_hud_surfaces(std::iter::once(PublishedHudSurface {
        owner: OWNER,
        revision,
        surface,
        authority: ActorAuthority::default(),
    }));
}

/// A panel embedding `controls` side by side.
fn embedding(controls: &[&str]) -> ui::mod_panel::Panel {
    let mut panel = crate::test_support::sample_panel();
    panel.controls.clear();
    let nodes: Vec<Value> = controls
        .iter()
        .enumerate()
        .map(|(index, control)| {
            json!({format!("preview_{index}"):{"type":"custom",
                "renderer":"cinnabar_hud_control","control":control,"size":[40,40],
                "offset":[50 * index,0],"anchor_from":"top_left","anchor_to":"top_left"}})
        })
        .collect();
    panel.surface = serde_json::from_value(json!({"screen":"menu.root",
        "document":json!({"namespace":"menu","root":{"type":"panel","controls":nodes}})
            .to_string()}))
    .unwrap();
    panel
}

/// Paints the HUD and the open panel, then sets `#shade` to `shade` and paints again, so
/// every cached control paints once more and a rebuilt one stands out.
fn paint_twice(ui: &mut ModUi, harness: &mut Harness, revision: u64, shade: f64) {
    harness.paint(ui, false);
    harness.paint(ui, true);
    let mut shaded = (*ui.hud_surfaces.layers[0].surface).clone();
    shaded
        .bindings
        .insert("#shade".into(), SurfaceValue::Number(shade));
    publish(ui, revision, &Arc::new(shaded));
    harness.paint(ui, false);
    harness.paint(ui, true);
}

/// The current surface of the only layer.
fn current(ui: &ModUi) -> Arc<Surface> {
    Arc::clone(&ui.hud_surfaces.layers[0].surface)
}

/// Embedded paint counts, by control.
fn embeds(ui: &ModUi) -> Vec<(String, Option<usize>)> {
    ui.hud_surface_embed_paints(OWNER).into_iter().collect()
}

#[test]
fn a_replaced_definition_rebuilds_only_the_parts_and_embeds_reading_it() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    let first = surface();
    publish(&mut ui, 1, &first);
    ui.set_mod_panel(
        OWNER,
        Some(&embedding(&["preview_a", "preview_c", "card_b"])),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    paint_twice(&mut ui, &mut harness, 2, 0.5);
    assert_eq!(ui.hud_surface_paints(OWNER), [2, 2, 2]);
    assert_eq!(
        embeds(&ui),
        ["card_b", "preview_a", "preview_c"]
            .map(|name| (name.to_owned(), Some(2)))
            .to_vec()
    );
    harness.paint(&mut ui, false);
    let before = harness.painted();
    let wider = patched(
        &current(&ui),
        json!({"controls":{"card_a":card(60,[10,10])}}),
    );
    publish(&mut ui, 3, &wider);
    harness.paint(&mut ui, false);
    harness.paint(&mut ui, true);
    assert_eq!(
        ui.hud_surface_paints(OWNER),
        [1, 2, 2],
        "only the root card instancing card_a repaints"
    );
    assert_eq!(
        embeds(&ui),
        [("card_b", 2), ("preview_a", 1), ("preview_c", 2)]
            .map(|(name, paints)| (name.to_owned(), Some(paints)))
            .to_vec(),
        "only the preview instancing card_a is rebuilt"
    );
    harness.paint(&mut ui, false);
    assert_ne!(harness.painted(), before, "the wider card draws");
    let unrelated = patched(
        &wider,
        json!({"controls":{"preview_c":card(10,[0,0]),"extra":{"type":"panel"}}}),
    );
    publish(&mut ui, 4, &unrelated);
    harness.paint(&mut ui, false);
    harness.paint(&mut ui, true);
    assert_eq!(
        ui.hud_surface_paints(OWNER),
        [1, 2, 2],
        "the root keeps every part"
    );
    assert_eq!(embeds(&ui)[2], ("preview_c".to_owned(), Some(1)));
    let removed = patched(&unrelated, json!({"controls":{"preview_a":null}}));
    publish(&mut ui, 5, &removed);
    harness.paint(&mut ui, true);
    assert_eq!(
        embeds(&ui)[1],
        ("preview_a".to_owned(), None),
        "it no longer draws"
    );
}

#[test]
fn a_changed_root_rebuilds_its_parts_and_keeps_unaffected_embeds() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    let first = surface();
    publish(&mut ui, 1, &first);
    ui.set_mod_panel(OWNER, Some(&embedding(&["preview_a", "card_b"])))
        .unwrap();
    ui.set_mod_panel_open(true);
    paint_twice(&mut ui, &mut harness, 2, 0.5);
    let root = |width| {
        json!({"type":"panel","size":["100%","100%"],"layer":1,"controls":[
            {"a@hud.card_a":{}},{"b@hud.card_b":{}},{"c":card(width,[10,70])}]})
    };
    let next = patched(&current(&ui), json!({"controls":{"root":root(20)}}));
    publish(&mut ui, 3, &next);
    harness.paint(&mut ui, false);
    harness.paint(&mut ui, true);
    assert_eq!(
        ui.hud_surface_paints(OWNER),
        [1, 1, 1],
        "new root properties rebuild every part"
    );
    assert_eq!(
        embeds(&ui),
        ["card_b", "preview_a"]
            .map(|name| (name.to_owned(), Some(2)))
            .to_vec()
    );
    paint_twice(&mut ui, &mut harness, 4, 0.25);
    assert_eq!(ui.hud_surface_paints(OWNER), [2, 2, 2]);
    let next = patched(&current(&ui), json!({"controls":{"root":root(30)}}));
    publish(&mut ui, 5, &next);
    harness.paint(&mut ui, false);
    assert_eq!(
        ui.hud_surface_paints(OWNER),
        [2, 2, 1],
        "a changed root child rebuilds alone"
    );
}

/// A rebuilt control that floats on a projection keeps moving its cached artwork there.
#[test]
fn a_rebuilt_floating_control_keeps_its_projected_place() {
    let tag = |width: u32| {
        json!({"type":"panel","size":[0,0],"anchor_from":"top_left","anchor_to":"top_left",
            "bindings":[{"binding_name":"#tag","binding_name_override":"#offset"},
                {"binding_name":"#tag_visible","binding_name_override":"#visible"}],
            "controls":[{"box":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
                "size":[width,10],"color":[1,0,0,1]}}]})
    };
    let first: Arc<Surface> = Arc::new(
        serde_json::from_value(json!({"screen":"hud.root",
            "document":json!({"namespace":"hud","tag_card":tag(20),
                "root":{"type":"panel","size":["100%","100%"],"controls":[
                    {"tag@hud.tag_card":{}},{"spacer":{"type":"panel","size":[1,1]}}]}})
            .to_string(),
            "bindings":{"#tag":{"project":{"world":[2,1,-10]}}}}))
        .unwrap(),
    );
    let camera =
        glam::camera::rh::proj::directx::perspective(1.0, 16. / 9., 0.1, 1000.).to_cols_array();
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    publish(&mut ui, 1, &first);
    harness.paint(&mut ui, false);
    ui.set_hud_projection(Some(camera), &|_| None);
    harness.paint(&mut ui, false);
    let before = harness.painted();
    let wider = patched(&first, json!({"controls":{"tag_card":tag(40)}}));
    publish(&mut ui, 2, &wider);
    harness.paint(&mut ui, false);
    let after = harness.painted();
    let centre = |rect: [f32; 4]| [(rect[0] + rect[2]) * 0.5, (rect[1] + rect[3]) * 0.5];
    assert_eq!(centre(after[0]), centre(before[0]), "{before:?} {after:?}");
    assert!(after[0][2] - after[0][0] > before[0][2] - before[0][0]);
}

/// The names of the layer's built embeddable controls.
fn built(layer: &Layer) -> Vec<String> {
    layer.embedded.keys().cloned().collect()
}

#[test]
fn embedded_controls_make_way_least_recently_drawn_first() {
    let count = MAX_EMBEDDED + 1;
    let mut document = serde_json::Map::new();
    document.insert("namespace".into(), json!("hud"));
    document.insert("root".into(), json!({"type":"panel"}));
    for index in 0..count {
        document.insert(format!("e{index}"), json!({"type":"panel","size":[4,4]}));
    }
    let surface: Arc<Surface> = Arc::new(
        serde_json::from_value(json!({"screen":"hud.root",
            "document":Value::Object(document).to_string()}))
        .unwrap(),
    );
    let mut layer = Layer::new(OWNER, 1, surface, ActorAuthority::default(), [640., 360.]);
    layer.begin_embeds();
    for index in 0..MAX_EMBEDDED {
        assert!(layer.embedded(&format!("e{index}")).is_some());
    }
    let last = format!("e{MAX_EMBEDDED}");
    assert!(
        layer.embedded(&last).is_none(),
        "controls drawn this frame stay"
    );
    assert_eq!(layer.embedded.len(), MAX_EMBEDDED);
    layer.begin_embeds();
    for index in (0..MAX_EMBEDDED).filter(|index| *index != 7) {
        layer.embedded(&format!("e{index}"));
    }
    layer.begin_embeds();
    assert!(layer.embedded(&last).is_some());
    let names = built(&layer);
    assert_eq!(names.len(), MAX_EMBEDDED);
    assert!(
        !names.contains(&"e7".to_owned()),
        "e7 was drawn longest ago"
    );
    assert!(names.contains(&last));
    assert!(
        layer.embedded("e7").is_some(),
        "an evicted control rebuilds"
    );
    assert_eq!(built(&layer).len(), MAX_EMBEDDED);
}
