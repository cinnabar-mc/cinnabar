//! Which Shift-clicks on an engine-drawn container delete the slot's stack.

use json_ui::{Catalog, Context, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState};
use ui_engine::frame::EngineFrame;

use super::ScreenLayout;
use super::presses::engine_auto_place_destroys;
use crate::container_actions::InventoryCellHit;

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

/// A creative-style layout: three hotbar cells that delete on Shift-click above three main
/// inventory cells that move. The test names each row's collection.
const ROWS: &str = r#"{
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
      {"hotbar": {
        "type": "grid", "size": [60, 20], "grid_dimensions": [3, 1],
        "anchor_from": "top_left", "anchor_to": "top_left",
        "collection_name": "hotbar_items",
        "controls": [
          {"first@slots.creative_hotbar_slot_button": {"grid_position": [0, 0]}},
          {"second@slots.creative_hotbar_slot_button": {"grid_position": [1, 0]}},
          {"third@slots.creative_hotbar_slot_button": {"grid_position": [2, 0]}}
        ]
      }},
      {"inventory": {
        "type": "grid", "size": [60, 20], "grid_dimensions": [3, 1], "offset": [0, 25],
        "anchor_from": "top_left", "anchor_to": "top_left",
        "collection_name": "inventory_items",
        "controls": [
          {"first@slots.slot_button": {"grid_position": [0, 0]}},
          {"second@slots.slot_button": {"grid_position": [1, 0]}},
          {"third@slots.slot_button": {"grid_position": [2, 0]}}
        ]
      }}
    ]
  }
}"#;

/// One slot whose centre a small control without a cell covers, drawn on top of it.
const COVERED: &str = r#"{
  "namespace": "slots",
  "slot_button": {
    "type": "button", "size": [20, 20],
    "anchor_from": "top_left", "anchor_to": "top_left"
  },
  "screen": {
    "type": "screen",
    "controls": [
      {"slot@slots.slot_button": {"offset": [0, 0]}},
      {"cover": {
        "type": "button", "size": [8, 8], "offset": [6, 6],
        "anchor_from": "top_left", "anchor_to": "top_left"
      }}
    ]
  }
}"#;

/// One slot a wider control covers except for its leftmost pixel column.
const STRIP: &str = r#"{
  "namespace": "slots",
  "slot_button": {
    "type": "button", "size": [20, 20],
    "anchor_from": "top_left", "anchor_to": "top_left"
  },
  "screen": {
    "type": "screen",
    "controls": [
      {"slot@slots.slot_button": {"offset": [0, 0]}},
      {"cover": {
        "type": "button", "size": [19, 20], "offset": [1, 0],
        "anchor_from": "top_left", "anchor_to": "top_left"
      }}
    ]
  }
}"#;

fn frame() -> EngineFrame {
    frame_of(SCREEN)
}

fn frame_of(screen: &str) -> EngineFrame {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/slots.json", screen);
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

/// A cell whose centre is covered, or that shows only a one-pixel strip, is still listed
/// through a point a pointer can reach.
#[test]
fn layout_cells_reach_a_partly_covered_cell() {
    for fixture in [COVERED, STRIP] {
        covered_cell_is_listed(fixture);
    }
}

fn covered_cell_is_listed(fixture: &str) {
    let mut frame = frame_of(fixture);
    let mut hits = frame.hits.to_vec();
    for region in &mut hits {
        if region.name == "slot" {
            region.collection = Some("hotbar_items".to_owned());
            region.collection_index = Some(0);
        }
    }
    frame.hits = hits.into();
    let layout = ScreenLayout::Personal { book: false };
    assert_eq!(
        layout.hit(&frame, [10.0, 10.0]),
        None,
        "the cover owns the centre"
    );
    let cells = layout.cells(&frame);
    let (hit, point) = cells
        .iter()
        .copied()
        .find(|(hit, _)| *hit == InventoryCellHit::Player(0))
        .expect("the covered slot is listed");
    assert_eq!(layout.hit(&frame, point), Some(hit));
}

/// Every drawn cell is listed once with a point that presses it, so a named click takes the
/// pointer's route, including the creative hotbar's delete-on-Shift-click.
#[test]
fn layout_cells_invert_hits_and_keep_auto_place_routing() {
    let mut frame = frame_of(ROWS);
    // Bound item data assigns grid cells their collection; this fixture has none, so name
    // each row's cells the way the bound vanilla rows would.
    let mut hits = frame.hits.to_vec();
    for (index, region) in hits.iter_mut().enumerate() {
        let collection = if index < 3 {
            "hotbar_items"
        } else {
            "inventory_items"
        };
        region.collection = Some(collection.to_owned());
        region.collection_index = Some(index % 3);
    }
    frame.hits = hits.into();
    let layout = ScreenLayout::Personal { book: false };
    let cells = layout.cells(&frame);
    let mut hits: Vec<_> = cells.iter().map(|(hit, _)| *hit).collect();
    hits.sort_by_key(|hit| match hit {
        InventoryCellHit::Player(slot) => *slot,
        _ => u8::MAX,
    });
    assert_eq!(
        hits,
        [0, 1, 2, 9, 10, 11].map(InventoryCellHit::Player).to_vec()
    );
    for (hit, point) in cells {
        assert_eq!(layout.hit(&frame, point), Some(hit));
        let hotbar = matches!(hit, InventoryCellHit::Player(slot) if slot < 9);
        assert_eq!(engine_auto_place_destroys(&frame, point), hotbar, "{hit:?}");
    }
}

/// Many controls stacked over a cell leave its time bounded by the controls over it, and a
/// strip they all leave exposed is still found.
#[test]
fn layout_cells_scale_with_stacked_covers() {
    let mut frame = frame_of(STRIP);
    let mut hits = frame.hits.to_vec();
    let slot = hits
        .iter()
        .position(|region| region.name == "slot")
        .unwrap();
    hits[slot].collection = Some("hotbar_items".to_owned());
    hits[slot].collection_index = Some(0);
    let cover = hits
        .iter()
        .find(|region| region.name == "cover")
        .unwrap()
        .clone();
    hits.extend(std::iter::repeat_n(cover, 250));
    frame.hits = hits.into();
    let layout = ScreenLayout::Personal { book: false };
    let cells = layout.cells(&frame);
    let (_, point) = cells
        .iter()
        .find(|(hit, _)| *hit == InventoryCellHit::Player(0))
        .expect("the strip under every cover is found");
    assert!(point[0] < 1.0, "the point lies in the exposed strip");
}
