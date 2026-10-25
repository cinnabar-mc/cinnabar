//! Which Shift-clicks on an engine-drawn container delete the slot's stack.

use json_ui::{Catalog, Context, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState};
use ui_engine::frame::EngineFrame;

use super::presses::engine_auto_place_destroys;

struct NoArt;
impl TextMeasure for NoArt {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}
impl TextureSource for NoArt {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Slot buttons shaped like vanilla's: a prototype routing `button.menu_auto_place`
/// through `$button_auto_place`, a survival slot keeping its default, a creative hotbar
/// slot overriding it to `button.container_auto_destroy`, and a disabled creative slot.
const SCREEN: &str = r#"{
  "namespace": "slots",
  "slot_button": {
    "type": "button", "size": [20, 20],
    "anchor_from": "top_left", "anchor_to": "top_left",
    "$button_auto_place|default": "button.container_auto_place",
    "button_mappings": [
      {"from_button_id": "button.menu_select",
        "to_button_id": "button.container_take_all_place_all", "mapping_type": "pressed"},
      {"from_button_id": "button.menu_auto_place",
        "to_button_id": "$button_auto_place", "mapping_type": "pressed"}
    ]
  },
  "creative_hotbar_slot_button@slots.slot_button": {
    "$button_auto_place": "button.container_auto_destroy"
  },
  "screen": {
    "type": "screen",
    "controls": [
      {"survival_slot@slots.slot_button": {"offset": [0, 0]}},
      {"creative_slot@slots.creative_hotbar_slot_button": {"offset": [30, 0]}},
      {"disabled_slot@slots.creative_hotbar_slot_button": {"offset": [60, 0], "enabled": false}}
    ]
  }
}"#;

fn frame() -> EngineFrame {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/slots.json", SCREEN);
    let screen = json_ui::resolve(&catalog, "slots.screen", &Context::desktop())
        .control
        .expect("the screen resolves");
    let (laid, report) = json_ui::layout_with(
        &screen,
        [100.0, 40.0],
        &LayoutEnv {
            text: &NoArt,
            textures: &NoArt,
        },
        &ViewState::default(),
    );
    EngineFrame {
        hits: json_ui::hit_regions(&laid).into(),
        report,
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    }
}

#[test]
fn shift_clicks_on_creative_hotbar_slots_destroy_the_stack() {
    assert!(engine_auto_place_destroys(&frame(), [40.0, 10.0]));
}

#[test]
fn shift_clicks_elsewhere_move_the_stack() {
    let frame = frame();
    for (point, place) in [
        ([10.0, 10.0], "survival slot"),
        ([70.0, 10.0], "disabled creative slot"),
        ([10.0, 35.0], "empty screen"),
    ] {
        assert!(!engine_auto_place_destroys(&frame, point), "{place}");
    }
}
