//! Radio tabs a server pack draws over a storage screen's cells.

use std::collections::BTreeMap;

use json_ui::{
    Catalog, CollectionItem, Context, DataSource, EmptyLibrary, HitRegion, LayoutEnv, Scalar,
    TextMeasure, TextureMeta, TextureSource, ViewState, bind,
};

use super::ScreenLayout;
use super::storage_radio::select;
use crate::container_actions::{InventoryCellHit, Widget};
use crate::container_kinds::storage_kind;
use ui_engine::frame::EngineFrame;

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

/// Two tabs in cells 45 and 46 of the window, each showing a page through its `#toggle_state`,
/// shaped like a server pack's shop. `FIRST`, `SECOND` and `TAB` hold each way of indexing them.
const SCREEN: &str = r##"{
  "namespace": "shop",
  "tab": {
    "type": "toggle", "size": [20, 20],
    "anchor_from": "top_left", "anchor_to": "top_left",
    "radio_toggle_group": true, "toggle_name": "navigation_tab", TAB
    "property_bag": {"#toggle_state": true},
    "bindings": [{"binding_type": "global", "binding_name": "#toggle_state",
      "binding_name_override": "#toggle_state", "binding_condition": "visible"}],
    "button_mappings": [
      {"to_button_id": "button.menu_select", "from_button_id": "button.menu_select",
        "mapping_type": "pressed"}
    ]
  },
  "page": {
    "type": "panel", "size": [100, 100], "offset": [0, 130],
    "anchor_from": "top_left", "anchor_to": "top_left",
    "bindings": [{"binding_type": "view", "source_control_name": "$tab",
      "source_property_name": "#toggle_state", "target_property_name": "#visible"}]
  },
  "screen": {
    "type": "screen",
    "controls": [
      {"tabs": {
        "type": "grid", "size": [180, 120], "grid_dimensions": [9, 6],
        "anchor_from": "top_left", "anchor_to": "top_left",
        "collection_name": "container_items",
        "controls": [
          {"first@shop.tab": {"grid_position": [0, 5], FIRST "toggle_default_state": true}},
          {"second@shop.tab": {"grid_position": [1, 5] SECOND}}
        ]
      }},
      {"first_page@shop.page": {"$tab": "first"}},
      {"second_page@shop.page": {"$tab": "second"}}
    ]
  }
}"##;

/// One tab screen: its layout, whether each page shows and whether each tab reads checked.
struct Drawn {
    frame: EngineFrame,
    pages: [bool; 2],
    checked: [bool; 2],
}

fn frame(screen: &str, selected: &BTreeMap<u64, u16>, previous: Option<&[HitRegion]>) -> Drawn {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/shop.json", screen);
    let screen = json_ui::resolve(&catalog, "shop.screen", &Context::desktop())
        .control
        .expect("the screen resolves");
    let mut data = DataSource::new();
    data.set_strict(true);
    data.set_collection(
        "container_items",
        (0..54)
            .map(|_| CollectionItem::default().with("#hover_text", Scalar::Text("tab".into())))
            .collect(),
    );
    if let Some(hits) = previous {
        select(&mut data, hits, selected);
    }
    let bound = bind(&screen, &data, &EmptyLibrary);
    let (laid, report) = json_ui::layout_with(
        &bound,
        [300.0, 300.0],
        &LayoutEnv {
            text: &NoArt,
            textures: &NoArt,
        },
        &ViewState::default(),
    );
    let visible = |name: &str| {
        laid.children
            .iter()
            .find(|child| child.control.name == name)
            .is_some_and(|child| child.control.properties.get("visible") != Some(&false.into()))
    };
    let pages = [visible("first_page"), visible("second_page")];
    let hits = json_ui::hit_regions(&laid);
    let checked = |name: &str| {
        hits.iter()
            .find(|hit| hit.key.contains(name))
            .and_then(|hit| hit.checked)
            .unwrap_or_else(|| panic!("tab {name} has a toggle region"))
    };
    let checked = [checked("first"), checked("second")];
    Drawn {
        frame: EngineFrame {
            hits: hits.into(),
            report,
            cancel_target: None,
            origin: [0.0; 2],
            scale: 1.0,
            panel: None,
            edit_texts: Vec::new(),
            top: Vec::new(),
        },
        pages,
        checked,
    }
}

/// Opens `screen`, presses the second tab and returns the index the press selected.
fn switches_tabs(screen: &str) -> u16 {
    let opened = frame(screen, &BTreeMap::new(), None);
    assert_eq!(opened.pages, [true, false], "the default tab's page shows");
    assert_eq!(
        opened.checked,
        [true, false],
        "the default tab reads checked"
    );

    let layout = ScreenLayout::Station(storage_kind(549, None));
    let Some(InventoryCellHit::Widget(Widget::StorageRadio { group, index })) =
        layout.hit(&opened.frame, [30.0, 110.0])
    else {
        panic!("a press on the second tab selects it rather than taking cell 46");
    };

    let selected = BTreeMap::from([(group, index)]);
    let pressed = frame(screen, &selected, Some(&opened.frame.hits));
    assert_eq!(pressed.pages, [false, true], "the pressed tab's page shows");
    assert_eq!(
        pressed.checked,
        [false, true],
        "the pressed tab reads checked"
    );
    index
}

/// `SCREEN` with its tabs indexed by `first` and `second`, with `tab` on each.
fn indexed(tab: &str, first: &str, second: &str) -> String {
    SCREEN
        .replace("TAB", tab)
        .replace("FIRST", first)
        .replace("SECOND", second)
}

#[test]
fn storage_tabs_start_on_their_default_and_switch_when_pressed() {
    let screen = indexed(
        "",
        r#""toggle_group_forced_index": 0,"#,
        r#", "toggle_group_forced_index": 1"#,
    );
    assert_eq!(switches_tabs(&screen), 1);
}

/// A negative or absent forced index reads the named collection, as `ToggleComponent::_setChecked`
/// reports it.
#[test]
fn storage_tabs_without_a_forced_index_use_their_cell() {
    let tab = r#""toggle_grid_collection_name": "container_items","#;
    assert_eq!(switches_tabs(&indexed(tab, "", "")), 46);
    let negative = indexed(
        tab,
        r#""toggle_group_forced_index": -1,"#,
        r#", "toggle_group_forced_index": -1"#,
    );
    assert_eq!(switches_tabs(&negative), 46);
}
