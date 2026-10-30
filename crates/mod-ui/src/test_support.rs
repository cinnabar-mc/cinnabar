//! Views of retained surface state and pure layout helpers for host-level tests.

use std::sync::Arc;

use json_ui::Catalog;
use serde_json::Value;
use ui::{
    mod_hud::Card,
    mod_panel::{Control, Panel},
};
use ui_engine::frame::EngineFrame;

/// The personal panel's catalog and last drawn frame.
pub struct ModPanelState<'a> {
    pub catalog: Option<&'a Arc<Catalog>>,
    pub frame: Option<&'a EngineFrame>,
    /// Bind and layout passes its cached screen ran.
    pub passes: usize,
    /// Static root resolutions performed on the renderer's frame path.
    #[cfg(feature = "test-support")]
    pub resolves: usize,
}

/// A card's size in GUI units.
pub fn card_dimensions(card: &Card) -> [f64; 2] {
    crate::mod_widgets::template::dimensions(card)
}

/// A card's top-left in a `viewport` of GUI units.
pub fn card_origin(card: &Card, viewport: [f64; 2]) -> [f64; 2] {
    crate::mod_widgets::template::origin(card, viewport)
}

/// The JSON-UI control authored for `card`, numbering its rows from `row_index`.
pub fn card_control(card: &Card, card_index: usize, row_index: &mut usize) -> Value {
    crate::mod_widgets::template::card(card, card_index, row_index)
}

/// Whether two panels keep one catalog.
pub fn panel_same_shape(a: &Panel, b: &Panel) -> bool {
    crate::mod_panel::same_shape(a, b)
}

/// The colours the panel's popup widgets paint with.
pub fn panel_palette(panel: &Panel) -> crate::mod_panel::Palette {
    crate::mod_panel::Palette::for_panel(panel)
}

/// A pressed-action button of `size` at `offset` holding `controls`.
fn sample_button(action: &str, size: [u32; 2], offset: [u32; 2], controls: Vec<Value>) -> Value {
    serde_json::json!({"type":"button","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left",
        "button_mappings":[{"from_button_id":"button.menu_select",
            "to_button_id":action,"mapping_type":"pressed"}],"controls":controls})
}

/// A rounded rectangle whose alpha follows `binding`, filling its parent.
fn sample_feedback(binding: &str) -> Value {
    serde_json::json!({"type":"custom","renderer":"cinnabar_rounded_rectangle",
        "size":["100%","100%"],"color":[1,1,1,1],
        "bindings":[{"binding_name":binding,"binding_name_override":"#alpha"}]})
}

/// A surface with a close button and one 120 by 20 row per control, stacked from the top
/// left, each routed to its control, showing its value and native hover feedback. Toggles
/// show their eased progress and sliders their fill and a value field; text controls open
/// the native field editor.
pub fn sample_surface(controls: &[Control]) -> ui::mod_panel::Surface {
    let mut close = sample_button("mod.close", [20, 20], [0, 8], Vec::new());
    close["anchor_from"] = "top_right".into();
    close["anchor_to"] = "top_right".into();
    close["offset"] = serde_json::json!([-8, 8]);
    let close = serde_json::json!({ "close": close });
    let rows = controls.iter().enumerate().flat_map(|(index, control)| {
        let y = 8 + index as u32 * 24;
        let row = |field: &str| format!("#row_{index}_{field}");
        let mut contents = vec![
            serde_json::json!({"hover":sample_feedback(&row("hover"))}),
            serde_json::json!({"value":{"type":"label","text":row("value"),
                "bindings":[{"binding_name":row("value")}]}}),
        ];
        match control {
            Control::Toggle { .. } => contents.push(serde_json::json!({"knob":{
                "type":"custom","renderer":"cinnabar_rounded_rectangle","size":[6,6],
                "anchor_from":"top_left","anchor_to":"top_left","color":[1,1,1,1],
                "bindings":[{"binding_name":row("knob_offset"),"binding_name_override":"#offset"},
                    {"binding_name":row("toggle_progress"),"binding_name_override":"#alpha"}]}})),
            Control::Slider { .. } => {
                contents.push(serde_json::json!({"fill":sample_feedback(&row("fill"))}))
            }
            _ => {}
        }
        let action = if matches!(control, Control::Text { .. }) {
            format!("mod.edit:{index}")
        } else {
            format!("mod.control:{index}")
        };
        let mut buttons = vec![serde_json::json!({format!("row_{index}"):
            sample_button(&action, [120, 20], [8, y], contents)})];
        if matches!(control, Control::Slider { .. }) {
            buttons.push(serde_json::json!({format!("field_{index}"):
                sample_button(&format!("mod.edit:{index}"), [48, 20], [136, y], Vec::new())}));
        }
        buttons
    });
    let rows: Vec<Value> = std::iter::once(close).chain(rows).collect();
    ui::mod_panel::Surface {
        screen: "sample.root".into(),
        document: serde_json::json!({"namespace":"sample","root":{"type":"panel",
            "size":["100%","100%"],"controls":rows}})
        .to_string(),
        bindings: Default::default(),
        transitions: Default::default(),
        collections: Default::default(),
    }
}

/// A panel with one control of each kind and a [`sample_surface`].
pub fn sample_panel() -> Panel {
    let controls = vec![
        Control::Toggle {
            id: "enabled".into(),
            label: "Enabled".into(),
            value: false,
        },
        Control::Slider {
            id: "strength".into(),
            label: "Strength".into(),
            value: 35.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
        },
        Control::Choice {
            id: "mode".into(),
            label: "Activation".into(),
            index: 0,
            options: vec!["Continuous".into(), "While clicking".into()],
        },
        Control::Button {
            id: "binding".into(),
            label: "Toggle key".into(),
        },
    ];
    Panel {
        surface: sample_surface(&controls),
        reference_size: None,
        toggle_key: "ShiftRight".into(),
        controls,
        widget_colors: None,
        capture_key: false,
        show_gameplay_hud: false,
        motion: false,
    }
}
