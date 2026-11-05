//! Which presses on an engine-drawn container drop the held stack.

use json_ui::{Catalog, Context, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState};
use ui_engine::frame::EngineFrame;

use super::presses::{HeldStackPress, engine_held_stack_press};

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

/// A screen shaped like vanilla's: the screen maps presses to a cursor drop,
/// two `common.root_panel`-style sections claim them, a plain panel and a
/// `drop_item_panel` sit beside them, and an item scroll panel destroys. Below them, a
/// disabled item panel, an item panel under a disabled ancestor, and a section holding a
/// scroll view that maps its own press to a cursor drop.
const SCREEN: &str = r#"{
  "namespace": "drop",
  "section": {
    "type": "input_panel", "size": [40, 40],
    "anchor_from": "top_left", "anchor_to": "top_left",
    "button_mappings": [{"from_button_id": "button.menu_select",
      "to_button_id": "button.container_reset_held", "mapping_type": "pressed"}]
  },
  "screen": {
    "type": "screen",
    "button_mappings": [{"from_button_id": "button.menu_select",
      "to_button_id": "button.cursor_drop_all", "mapping_type": "pressed"}],
    "controls": [
      {"cursor_visual": {"type": "button", "size": [16, 16], "layer": -10, "follows_cursor": true, "always_handle_pointer": true}},
      {"root_panel@drop.section": {"offset": [0, 0]}},
      {"recipe_book@drop.section": {"offset": [50, 0]}},
      {"background": {"type": "panel", "size": [40, 40],
        "anchor_from": "top_left", "anchor_to": "top_left", "offset": [100, 0]}},
      {"drop_item_panel": {"type": "input_panel", "size": [40, 40],
        "anchor_from": "top_left", "anchor_to": "top_left", "offset": [150, 0],
        "button_mappings": [{"from_button_id": "button.menu_select",
          "to_button_id": "button.cursor_drop_all", "mapping_type": "pressed"}]}},
      {"item_scroll_panel": {"type": "input_panel", "size": [40, 40],
        "anchor_from": "top_left", "anchor_to": "top_left", "offset": [50, 50],
        "button_mappings": [{"from_button_id": "button.menu_select",
          "to_button_id": "button.destroy_selection", "mapping_type": "pressed"}],
        "controls": [{"view": {"type": "scroll_view", "size": ["100%", "100%"]}}]}},
      {"disabled_item_panel": {"type": "input_panel", "size": [40, 40], "enabled": false,
        "anchor_from": "top_left", "anchor_to": "top_left", "offset": [100, 50],
        "button_mappings": [{"from_button_id": "button.menu_select",
          "to_button_id": "button.destroy_selection", "mapping_type": "pressed"}],
        "controls": [{"view": {"type": "scroll_view", "size": ["100%", "100%"]}}]}},
      {"disabled_section": {"type": "panel", "size": [40, 40], "enabled": false,
        "anchor_from": "top_left", "anchor_to": "top_left", "offset": [150, 50],
        "controls": [{"item_panel": {"type": "input_panel", "size": ["100%", "100%"],
          "button_mappings": [{"from_button_id": "button.menu_select",
            "to_button_id": "button.destroy_selection", "mapping_type": "pressed"}]}}]}},
      {"pack_section@drop.section": {"offset": [0, 50],
        "controls": [{"dropping_view": {"type": "scroll_view", "size": ["100%", "100%"],
          "button_mappings": [{"from_button_id": "button.menu_select",
            "to_button_id": "button.cursor_drop_all", "mapping_type": "pressed"}]}}]}}
    ]
  }
}"#;

fn frame() -> EngineFrame {
    frame_at(None)
}

/// Lays out the cursor visual over the requested pointer location.
fn frame_at(pointer: Option<[f64; 2]>) -> EngineFrame {
    frame_from(SCREEN, pointer)
}

/// Resolves an original screen fixture with the supplied cursor position.
fn frame_from(source: &str, pointer: Option<[f64; 2]>) -> EngineFrame {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/drop.json", source);
    let screen = json_ui::resolve(&catalog, "drop.screen", &Context::desktop())
        .control
        .expect("the screen resolves");
    let (laid, report) = json_ui::layout_with(
        &screen,
        [200.0, 100.0],
        &LayoutEnv {
            text: &NoArt,
            textures: &NoArt,
        },
        &ViewState {
            pointer,
            ..Default::default()
        },
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
fn presses_on_any_section_keep_the_held_stack() {
    let frame = frame();
    assert_eq!(
        engine_held_stack_press(&frame, [10.0, 10.0]),
        None,
        "root panel"
    );
    assert_eq!(
        engine_held_stack_press(&frame, [60.0, 10.0]),
        None,
        "recipe book"
    );
}

#[test]
fn presses_off_the_sections_drop_the_held_stack() {
    let frame = frame();
    for (point, place) in [
        ([110.0, 10.0], "plain panel"),
        ([160.0, 10.0], "drop panel"),
        ([60.0, 95.0], "empty screen"),
    ] {
        assert_eq!(
            engine_held_stack_press(&frame, point),
            Some(HeldStackPress::Drop),
            "{place}"
        );
    }
}

/// Vanilla's creative and recipe book item grid maps a press on its empty background,
/// under the grid's scroll view, to `button.destroy_selection`.
#[test]
fn presses_on_an_item_scroll_panel_destroy_the_held_stack() {
    assert_eq!(
        engine_held_stack_press(&frame(), [60.0, 60.0]),
        Some(HeldStackPress::Destroy)
    );
}

/// A disabled item panel, or one under a disabled ancestor, claims the press without
/// destroying the held stack or letting the press through to the screen's drop.
#[test]
fn presses_on_a_disabled_item_panel_keep_the_held_stack() {
    let frame = frame();
    for (point, place) in [
        ([110.0, 60.0], "disabled panel"),
        ([160.0, 60.0], "disabled ancestor"),
    ] {
        assert_eq!(engine_held_stack_press(&frame, point), None, "{place}");
    }
}

/// A scroll view that maps its own press keeps that mapping instead of passing the press to
/// the section under it, which would keep the stack.
#[test]
fn a_scroll_view_with_its_own_drop_mapping_drops_the_held_stack() {
    assert_eq!(
        engine_held_stack_press(&frame(), [10.0, 60.0]),
        Some(HeldStackPress::Drop)
    );
}

/// A cursor visual must leave drops, panel ownership and creative destruction intact.
#[test]
fn unmapped_cursor_buttons_do_not_block_held_stack_presses() {
    for (point, expected) in [
        ([110.0, 10.0], Some(HeldStackPress::Drop)),
        ([160.0, 10.0], Some(HeldStackPress::Drop)),
        ([10.0, 10.0], None),
        ([60.0, 10.0], None),
        ([60.0, 60.0], Some(HeldStackPress::Destroy)),
        ([110.0, 60.0], None),
    ] {
        assert_eq!(
            engine_held_stack_press(&frame_at(Some(point)), point.map(|v| v as f32)),
            expected
        );
    }
}

/// The installed inventory templates include pointer-following held-item buttons.
#[test]
fn installed_inventory_background_allows_cursor_drops() {
    let Some(carrier) = ui_engine::test_support::carrier() else {
        return;
    };
    let catalog = Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (file.path.as_ref(), file.bytes.as_ref())),
    )
    .unwrap();
    let context = crate::recipe_book::context(Context::desktop());
    for reference in ["crafting.inventory_screen", "chest.small_chest_screen"] {
        let screen = json_ui::resolve(&catalog, reference, &context)
            .control
            .unwrap();
        let mut data = json_ui::DataSource::new();
        super::survival_globals(&mut data, "Inventory");
        let bound = json_ui::bind(
            &screen,
            &data,
            &json_ui::CatalogLibrary {
                catalog: &catalog,
                context: &context,
            },
        );
        let point = [430.0, 125.0];
        let (laid, report) = json_ui::layout_with(
            &bound,
            [480.0, 270.0],
            &LayoutEnv {
                text: &NoArt,
                textures: &NoArt,
            },
            &ViewState {
                pointer: Some(point),
                ..Default::default()
            },
        );
        let frame = EngineFrame {
            hits: json_ui::hit_regions(&laid).into(),
            report,
            cancel_target: None,
            origin: [0.0; 2],
            scale: 1.0,
            panel: None,
            edit_texts: Vec::new(),
            top: Vec::new(),
        };
        assert_eq!(
            engine_held_stack_press(&frame, point.map(|v| v as f32)),
            Some(HeldStackPress::Drop),
            "{reference}"
        );
    }
}

/// Cursor-following controls with a press mapping retain their own action.
#[test]
fn mapped_cursor_buttons_keep_their_press() {
    let screen = SCREEN.replace(
        "\"always_handle_pointer\": true}",
        r#""always_handle_pointer": true, "button_mappings": [{"from_button_id": "button.menu_select", "to_button_id": "button.destroy_selection", "mapping_type": "pressed"}]}"#,
    );
    let point = [110.0, 10.0];
    assert_eq!(
        engine_held_stack_press(&frame_from(&screen, Some(point)), point.map(|v| v as f32)),
        Some(HeldStackPress::Destroy)
    );
}
