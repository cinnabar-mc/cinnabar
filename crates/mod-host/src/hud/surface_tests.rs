use super::super::cinnabar::extension::hud as wit;
use super::super::{ModGrants, State};
use super::surface::{
    self, MAX_HUD_SURFACE_JSON_BYTES, MAX_MEASURED_TEXT_BYTES, MAX_TEXT_MEASURES,
};
use serde_json::json;
use std::sync::Arc;
use ui::mod_hud::{ElementState, HudViewport};

/// A state with `grants` and a fresh import budget.
fn state(grants: ModGrants) -> State {
    State::new(grants, String::new(), Default::default())
}

fn hud() -> ModGrants {
    ModGrants {
        hud: true,
        ..Default::default()
    }
}

/// A HUD surface whose root holds one label bound to `#text` and the given bindings.
fn surface(bindings: serde_json::Value) -> String {
    json!({
        "screen":"hud.root",
        "document":json!({"namespace":"hud","root":{"type":"panel","controls":[
            {"text":{"type":"label","text":"#text","bindings":[{"binding_name":"#text"}]}},
            {"tag":{"type":"panel","size":[0,0],"bindings":[
                {"binding_name":"#tag","binding_name_override":"#offset"},
                {"binding_name":"#tag_visible","binding_name_override":"#visible"}]}}
        ]}}).to_string(),
        "bindings":bindings,
    })
    .to_string()
}

/// Sets a surface, commits it and returns whether the guest saw success.
fn set(state: &mut State, json: String) -> bool {
    state.writes = 0;
    let ok = surface::set(state, json).unwrap().is_ok();
    state.hud.commit();
    ok
}

#[test]
fn surface_admission_bounds_size_shape_and_input() {
    let mut state = state(hud());
    assert!(!set(&mut state, "x".repeat(MAX_HUD_SURFACE_JSON_BYTES + 1)));
    assert!(!set(&mut state, "{".into()));
    assert!(!set(&mut state, json!({"screen":"hud.root"}).to_string()));
    let button = json!({"screen":"hud.root","document":json!({"namespace":"hud","root":{
        "type":"button","button_mappings":[{"from_button_id":"button.menu_select",
        "to_button_id":"mod.close","mapping_type":"pressed"}]}}).to_string()});
    assert!(
        !set(&mut state, button.to_string()),
        "HUD surfaces have no input"
    );
    assert!(state.hud.surface.is_none());
    let many: serde_json::Map<_, _> = (0..ui::mod_panel::MAX_HUD_SURFACE_BINDINGS)
        .map(|i| (format!("#row_{i}"), json!(i)))
        .collect();
    assert!(set(&mut state, surface(json!(many))));
    assert_eq!(
        state.hud.surface.as_ref().unwrap().bindings.len(),
        ui::mod_panel::MAX_HUD_SURFACE_BINDINGS
    );
    let mut excess = many;
    excess.insert("#excess".into(), json!(1));
    assert!(!set(&mut state, surface(json!(excess))));
    assert_eq!(
        state.hud.surface.as_ref().unwrap().bindings.len(),
        ui::mod_panel::MAX_HUD_SURFACE_BINDINGS,
        "a rejected surface keeps the committed one"
    );
}

#[test]
fn updates_patch_the_staged_surface_and_need_one() {
    let mut state = state(hud());
    assert!(
        surface::update(&mut state, r##"{"binding_changes":{"#text":"a"}}"##.into())
            .unwrap()
            .is_err()
    );
    surface::set(&mut state, surface(json!({"#text":"a","#keep":1})))
        .unwrap()
        .unwrap();
    surface::update(&mut state, r##"{"binding_changes":{"#text":"b"}}"##.into())
        .unwrap()
        .unwrap();
    assert!(
        state.hud.surface.is_none(),
        "nothing publishes before commit"
    );
    state.hud.commit();
    let committed = Arc::clone(state.hud.surface.as_ref().unwrap());
    let revision = state.hud.surface_revision;
    assert_eq!(committed.bindings["#text"], json_value("b"));
    assert_eq!(committed.bindings["#keep"], json_value(1));
    state.writes = 0;
    assert!(
        surface::update(&mut state, r##"{"binding_changes":{"bad":1}}"##.into())
            .unwrap()
            .is_err()
    );
    assert!(
        surface::update(&mut state, r#"{"document":"{}"}"#.into())
            .unwrap()
            .is_err()
    );
    surface::update(&mut state, r##"{"bindings":{"#text":"c"}}"##.into())
        .unwrap()
        .unwrap();
    state.hud.commit();
    let replaced = state.hud.surface.as_ref().unwrap();
    assert_eq!(replaced.bindings.len(), 1);
    assert_ne!(state.hud.surface_revision, revision);
}

/// Removals name bound bindings, apply before the patch's other bindings and keep the
/// document and its index; a rejected removal keeps the committed surface.
#[test]
fn updates_remove_bound_bindings_before_setting_others() {
    let mut state = state(hud());
    assert!(set(
        &mut state,
        surface(json!({"#text":"a","#keep":1,"#tag":{"project":{"world":[0,0,0]}}}))
    ));
    let update = |state: &mut State, patch: serde_json::Value| {
        state.writes = 0;
        let result = surface::update(state, patch.to_string()).unwrap();
        state.hud.commit();
        result
    };
    let revision = state.hud.surface_revision;
    for invalid in [
        json!({"remove_bindings":["#missing"]}),
        json!({"remove_bindings":["#text","#text"]}),
        json!({"remove_bindings":["#text"],"binding_changes":{"#text":"b"}}),
        json!({"remove_bindings":["#keep"],"bindings":{"#keep":2}}),
    ] {
        assert!(update(&mut state, invalid.clone()).is_err(), "{invalid}");
    }
    assert_eq!(
        state.hud.surface_revision, revision,
        "rejected removals keep the committed surface"
    );
    let document = state.hud.surface.as_ref().unwrap().document.clone();
    update(
        &mut state,
        json!({"remove_bindings":["#text","#tag"],"binding_changes":{"#tag_visible":true}}),
    )
    .unwrap();
    let committed = Arc::clone(state.hud.surface.as_ref().unwrap());
    assert_eq!(
        committed.bindings.keys().collect::<Vec<_>>(),
        ["#keep", "#tag_visible"]
    );
    assert_eq!(
        committed.document, document,
        "the document still reads #text"
    );
    assert!(state.hud.index.describes(&committed));
}

#[test]
fn control_updates_replace_definitions_validated_as_a_whole_document() {
    let mut state = state(hud());
    surface::set(&mut state, surface(json!({"#text":"a"})))
        .unwrap()
        .unwrap();
    state.hud.commit();
    let document = |state: &State| -> serde_json::Value {
        serde_json::from_str(&state.hud.surface.as_ref().unwrap().document).unwrap()
    };
    let update = |state: &mut State, controls: serde_json::Value| {
        state.writes = 0;
        let result = surface::update(state, json!({"controls":controls}).to_string()).unwrap();
        state.hud.commit();
        result
    };
    update(
        &mut state,
        json!({"card":{"type":"label","text":"#text","bindings":[{"binding_name":"#text"}]}}),
    )
    .unwrap();
    assert_eq!(document(&state)["card"]["type"], "label");
    assert_eq!(
        document(&state)["root"]["controls"][0]["text"]["type"],
        "label"
    );
    let revision = state.hud.surface_revision;
    for invalid in [
        json!({"root":{"type":"panel","controls":[{"c@hud.missing":{}}]}}),
        json!({"card":{"type":"panel","controls":[{"again@hud.card":{}}]}}),
        json!({"tag":null}),
    ] {
        assert!(update(&mut state, invalid.clone()).is_err(), "{invalid}");
    }
    assert_eq!(
        state.hud.surface_revision, revision,
        "rejected updates keep the committed surface"
    );
    update(&mut state, json!({"card":null})).unwrap();
    assert!(document(&state).get("card").is_none());
}

/// Parses one binding value.
#[test]
fn admitted_documents_stay_indexed_for_later_control_updates() {
    let mut state = state(hud());
    assert!(set(&mut state, surface(json!({"#text":"a"}))));
    let committed = |state: &State| state.hud.surface.clone().unwrap();
    assert!(state.hud.index.describes(&committed(&state)));
    for offset in 0..3 {
        state.writes = 0;
        let root = json!({"type":"panel","controls":[{"text":{"type":"label","text":"#text",
            "offset":[offset, 0],"bindings":[{"binding_name":"#text"}]}}]});
        surface::update(&mut state, json!({"controls":{"root":root}}).to_string())
            .unwrap()
            .unwrap();
        state.hud.commit();
        let document: serde_json::Value =
            serde_json::from_str(&committed(&state).document).unwrap();
        assert_eq!(
            document["root"]["controls"][0]["text"]["offset"],
            json!([offset, 0])
        );
        assert!(
            state.hud.index.describes(&committed(&state)),
            "update {offset}"
        );
    }
    let last = committed(&state);
    assert!(set(&mut state, String::new()));
    assert!(state.hud.surface.is_none());
    assert!(!state.hud.index.describes(&last), "removal drops the index");
}

fn json_value(value: impl serde::Serialize) -> ui::mod_panel::SurfaceValue {
    serde_json::from_value(json!(value)).unwrap()
}

#[test]
fn actor_projections_need_actor_authority_and_world_points_do_not() {
    let actor = json!({"#tag":{"project":{"actor":7,"offset":[0,2,0]}}});
    let world = json!({"#tag":{"project":{"world":[1,2,3]}}});
    let mut state = state(hud());
    assert!(!set(&mut state, surface(actor.clone())));
    assert!(set(&mut state, surface(world)));
    state.writes = 0;
    let patch = json!({"binding_changes":actor}).to_string();
    assert!(surface::update(&mut state, patch.clone()).unwrap().is_err());
    for grants in [
        ModGrants {
            players: true,
            ..hud()
        },
        ModGrants {
            entities: true,
            ..hud()
        },
    ] {
        let mut state = super::super::State::new(grants, String::new(), Default::default());
        assert!(set(&mut state, surface(actor.clone())));
        state.writes = 0;
        assert!(surface::update(&mut state, patch.clone()).unwrap().is_ok());
    }
}

#[test]
fn hud_imports_are_denied_without_hud_authority() {
    let mut state = state(ModGrants::default());
    assert!(!set(&mut state, surface(json!({}))));
    assert!(
        surface::set_hidden(&mut state, vec![wit::VanillaElement::Sidebar])
            .unwrap()
            .is_err()
    );
    state.hud.viewport = Some(HudViewport {
        width: 1.,
        height: 1.,
        scale: 1.,
    });
    assert!(surface::viewport(&state).is_none());
    assert!(surface::elements(&state).is_err());
    assert!(surface::measure(&mut state, "x", wit::Font::Native).is_err());
}

/// Fixed widths: six units per character in either face.
struct Metrics;

impl super::HudTextMeasure for Metrics {
    fn width(&self, text: &str, _: ui::mod_panel::LabelFont) -> Option<f32> {
        Some(text.chars().count() as f32 * 6.)
    }
}

#[test]
fn text_measurement_is_bounded_per_callback_and_line() {
    let mut state = state(hud());
    assert!(surface::measure(&mut state, "abc", wit::Font::Native).is_err());
    state.hud.text = Some(Arc::new(Metrics));
    assert_eq!(
        surface::measure(&mut state, "abc", wit::Font::Sans),
        Ok(18.)
    );
    assert!(surface::measure(&mut state, "a\nb", wit::Font::Native).is_err());
    assert!(
        surface::measure(
            &mut state,
            &"x".repeat(MAX_MEASURED_TEXT_BYTES + 1),
            wit::Font::Native
        )
        .is_err()
    );
    for _ in 4..MAX_TEXT_MEASURES {
        surface::measure(&mut state, "", wit::Font::Native).unwrap();
    }
    assert!(surface::measure(&mut state, "", wit::Font::Native).is_err());
    state.hud.begin_callback();
    assert_eq!(surface::measure(&mut state, "", wit::Font::Native), Ok(0.));
}

#[test]
fn element_reads_follow_enum_order_and_hiding_replaces_the_set() {
    let mut state = state(hud());
    state.hud.elements[ui::mod_hud::VanillaElement::Hunger.index()] = ElementState {
        visible: true,
        bounds: Some([10., 20., 81., 9.]),
    };
    let elements = surface::elements(&state).unwrap();
    assert_eq!(
        elements.iter().map(|e| e.element).collect::<Vec<_>>(),
        [
            wit::VanillaElement::Crosshair,
            wit::VanillaElement::StatusEffects,
            wit::VanillaElement::Sidebar,
            wit::VanillaElement::Hunger
        ]
    );
    assert!(elements[3].visible);
    assert_eq!(elements[3].bounds.map(|r| r.width), Some(81.));
    surface::set_hidden(
        &mut state,
        vec![wit::VanillaElement::Sidebar, wit::VanillaElement::Sidebar],
    )
    .unwrap()
    .unwrap();
    state.hud.commit();
    assert_eq!(
        state.hud.hidden.iter().collect::<Vec<_>>(),
        [ui::mod_hud::VanillaElement::Sidebar]
    );
}

#[test]
fn panel_actor_projections_need_actor_authority() {
    use super::super::cinnabar::extension::panel::Host as _;
    let panel = json!({"toggle_key":"F10","surface":{"screen":"menu.root",
        "document":json!({"namespace":"menu","root":{"type":"panel"}}).to_string(),
        "bindings":{"#tag":{"project":{"actor":3,"offset":[0,2,0]}}}}})
    .to_string();
    let controls = ModGrants {
        controls: true,
        ..Default::default()
    };
    let mut denied = state(controls.clone());
    assert!(denied.set_content(panel.clone()).unwrap().is_err());
    let mut granted = state(ModGrants {
        entities: true,
        ..controls
    });
    assert!(granted.set_content(panel).unwrap().is_ok());
}

#[test]
fn rollback_restores_the_committed_surface_index_before_a_guest_patch() {
    let mut state = state(hud());
    assert!(set(&mut state, surface(json!({"#text":"published"}))));
    surface::set(&mut state, String::new()).unwrap().unwrap();
    state.hud.rollback();
    surface::update(
        &mut state,
        r##"{"binding_changes":{"#text":"recovered"}}"##.into(),
    )
    .unwrap()
    .unwrap();
    state.hud.commit();
    assert_eq!(
        state.hud.surface.as_ref().unwrap().bindings["#text"],
        ui::mod_panel::SurfaceValue::Text("recovered".into())
    );
}
