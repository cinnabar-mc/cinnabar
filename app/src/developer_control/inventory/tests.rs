use std::sync::Arc;

use client_ui::ui_runtime::{UiRuntime, flush_inventory_send};
use developer_control::protocol::{ClickButton, InventoryClick, InventoryContainer};
use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent, NetworkItemStack,
};
use ui_screens::container_actions::InventoryCellHit;

use super::*;

const SESSION: u64 = 4;
const STONE: &str = "minecraft:stone";

/// A server-authoritative player holding 32 stone in main inventory slot 9.
fn player_with_stone() -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(SESSION);
    let mut ui = UiRuntime::new(SESSION);
    ui.publish_local_runtime_id(&mut player, SESSION, 42)
        .unwrap();
    let stone = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == STONE)
        .unwrap()
        .clone();
    let ledger = player.inventory.ledger_mut();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&protocol::ItemRegistryEvent {
        entries: Arc::from([stone.clone()]),
    });
    let mut slots = vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[9] = NetworkItemStack {
        network_id: stone.network_id,
        count: 32,
        stack_network_id: 100,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::empty(),
    }));
    (player, ui)
}

/// Sends whatever the inventory queued this frame, as the network flush does.
fn flush(player: &mut player_state::PlayerState, ui: &mut UiRuntime) -> usize {
    let mut sent = 0;
    while flush_inventory_send(player, ui, 0, |_| {
        sent += 1;
        Ok::<_, ()>(())
    })
    .unwrap()
    {}
    sent
}

fn click(container: InventoryContainer, slot: u16, button: ClickButton) -> InventoryClick {
    InventoryClick {
        container,
        slot,
        button,
        shift: false,
    }
}

fn count(value: &Value) -> u64 {
    value["count"].as_u64().unwrap()
}

/// Opens like the inventory key, splits a stack and places one item, all predicted before the
/// server answers, then closes like Escape.
#[test]
fn clicks_predict_through_the_player_click_path_without_waiting_for_the_server() {
    let (mut player, mut ui) = player_with_stone();
    let right = |slot| click(InventoryContainer::Inventory, slot, ClickButton::Right);
    assert_eq!(
        click_cell(&mut player, &mut ui, None, right(9)).unwrap_err(),
        "no inventory screen is open; send inventory_open first"
    );

    open_screen(&mut player, &mut ui, None).unwrap();
    assert!(ui.inventory_open());
    assert_eq!(report(&player, &ui, None, true)["ready"], false);
    assert!(
        click_cell(&mut player, &mut ui, None, right(9))
            .unwrap_err()
            .starts_with("the client refused the click")
    );
    assert!(flush(&mut player, &mut ui) > 0, "the open request leaves");
    assert_eq!(report(&player, &ui, None, true)["ready"], true);
    open_screen(&mut player, &mut ui, None).unwrap();
    assert!(ui.inventory_open(), "opening an open screen keeps it open");

    click_cell(&mut player, &mut ui, None, right(9)).unwrap();
    click_cell(&mut player, &mut ui, None, right(10)).unwrap();
    let view = report(&player, &ui, None, true);
    assert_eq!(view["screen"], "inventory");
    assert_eq!(view["cursor"]["identifier"], STONE);
    assert_eq!(count(&view["cursor"]), 15);
    assert_eq!(count(&view["inventory"][9]), 16);
    assert_eq!(count(&view["inventory"][10]), 1);
    assert_eq!(view["inventory"][11], Value::Null);
    assert_eq!(view["inventory"].as_array().unwrap().len(), 36);
    assert_eq!(view["crafting"]["width"], 2);
    assert_eq!(view["crafting"]["input"].as_array().unwrap().len(), 4);
    assert_eq!(view["pending_requests"], 2, "nothing was answered");

    close_screen(&mut player, &mut ui).unwrap();
    assert!(!ui.inventory_open());
    assert_eq!(report(&player, &ui, None, true)["open"], false);
    assert_eq!(
        close_screen(&mut player, &mut ui).unwrap_err(),
        "no inventory screen is open"
    );
}

/// Each container resolves to the cell the player's pointer would hit, and only cells the
/// open screen draws are clickable.
#[test]
fn cells_resolve_against_the_open_screen_layout() {
    let (mut player, mut ui) = player_with_stone();
    let ledger = player.inventory.ledger();
    for (container, slot, hit) in [
        (
            InventoryContainer::Inventory,
            0,
            InventoryCellHit::Player(0),
        ),
        (
            InventoryContainer::Inventory,
            35,
            InventoryCellHit::Player(35),
        ),
        (InventoryContainer::Armor, 3, InventoryCellHit::Armor(3)),
        (InventoryContainer::Offhand, 0, InventoryCellHit::Offhand),
        (
            InventoryContainer::Container,
            2,
            InventoryCellHit::Storage(2),
        ),
        (
            InventoryContainer::CraftingInput,
            0,
            InventoryCellHit::Craft(28),
        ),
        (
            InventoryContainer::CraftingInput,
            3,
            InventoryCellHit::Craft(31),
        ),
        (
            InventoryContainer::CraftingOutput,
            0,
            InventoryCellHit::CraftOutput,
        ),
    ] {
        assert_eq!(
            cell_hit(ledger, container, slot),
            Some(hit),
            "{container:?}"
        );
        assert_eq!(container_of(hit), Some(container));
    }
    assert_eq!(cell_hit(ledger, InventoryContainer::Offhand, 1), None);
    assert_eq!(
        cell_hit(ledger, InventoryContainer::CraftingOutput, 1),
        None
    );
    assert_eq!(cell_hit(ledger, InventoryContainer::Inventory, 256), None);

    open_screen(&mut player, &mut ui, None).unwrap();
    flush(&mut player, &mut ui);
    let left = |container, slot| click(container, slot, ClickButton::Left);
    for (container, slot, error) in [
        (
            InventoryContainer::CraftingInput,
            4,
            "crafting_input slot 4 is not on the open inventory screen, which has 4 such slots",
        ),
        (
            InventoryContainer::Inventory,
            36,
            "inventory slot 36 is not on the open inventory screen, which has 36 such slots",
        ),
        (
            InventoryContainer::Container,
            0,
            "container slot 0 is not on the open inventory screen, which has 0 such slots",
        ),
    ] {
        assert_eq!(
            click_cell(&mut player, &mut ui, None, left(container, slot)).unwrap_err(),
            error
        );
    }
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}

/// While a server form takes the player's input, the endpoint refuses to open the inventory,
/// and an inventory opened beneath the form does not take clicks, as with the pointer.
#[test]
fn a_form_in_front_takes_the_inventory_input() {
    let (mut player, mut ui) = player_with_stone();
    let top = |player: &player_state::PlayerState, ui: &UiRuntime| {
        crate::ui_runtime::input_owner::top_scene(ui, player, None)
    };
    assert_eq!(top(&player, &ui), Scene::Gameplay);
    client_ui::test_support::pack_harness::admit_action_form(&mut ui, &mut player, "Form", &["OK"]);
    assert_ne!(
        top(&player, &ui),
        Scene::Gameplay,
        "open refuses under the form"
    );
    open_screen(&mut player, &mut ui, None).unwrap();
    flush(&mut player, &mut ui);
    assert_ne!(
        top(&player, &ui),
        Scene::Container,
        "clicks refuse under the form"
    );
}

/// A player mod's view over the open inventory holds the keys and pointer, so the checks the
/// endpoint shares with Escape refuse closing it, though the inventory scene is still on top.
#[test]
fn a_mod_view_over_the_inventory_takes_its_input() {
    use mod_ui::mod_screens::ModScreensInput;
    use server_experience::screen;
    let Some(mut presentation) = client_ui::test_support::engine_presentation() else {
        eprintln!("skipping mod view input test: missing installed UI carrier; make assets");
        return;
    };
    let (mut player, mut ui) = player_with_stone();
    open_screen(&mut player, &mut ui, None).unwrap();
    flush(&mut player, &mut ui);
    let accepts = crate::ui_runtime::interaction::inventory::accepts_menu_cancel;
    assert!(accepts(&ui, Some(&presentation)));
    let template = serde_json::json!({"namespace":"view_test",
        "view": {"type":"panel", "size":["100%","100%"]}});
    let files = Arc::new(screen::Files {
        namespace: "view_test".into(),
        templates: [(
            "ui/view.json".into(),
            serde_json::to_vec(&template).unwrap(),
        )]
        .into(),
        textures: Vec::new(),
    });
    let view = "ui/view.json".to_owned();
    let data = screen::Modal::default();
    presentation.set_mod_screens(Some(ModScreensInput {
        id: "view_test",
        files: &files,
        overlay: None,
        view: Some(&view),
        focus: None,
        data: &data,
    }));
    presentation
        .build(
            &player,
            &ui,
            0,
            [1920, 1080],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(presentation.mod_ui().mod_view_shown());
    assert_eq!(
        crate::ui_runtime::input_owner::top_scene(&ui, &player, None),
        Scene::Container
    );
    assert!(!accepts(&ui, Some(&presentation)));
}

/// A player mod's panel sits outside the scene stack but holds the player's input, so the
/// endpoint refuses inventory commands while it is open, as the ordinary input paths do.
#[test]
fn an_open_mod_panel_holds_the_inventory_input() {
    let Some(presentation) = client_ui::test_support::engine_presentation() else {
        eprintln!("skipping mod panel input test: missing installed UI carrier; make assets");
        return;
    };
    let mut world = World::new();
    world.insert_resource(UiPresentationResource(presentation));
    assert!(!presentation_holds_input(&world));
    let panel = mod_ui::test_support::sample_panel();
    let mut view = world.resource_mut::<UiPresentationResource>();
    view.0
        .mod_ui_mut()
        .set_mod_panel(mod_ui::PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    view.0.mod_ui_mut().set_mod_panel_open(true);
    assert!(presentation_holds_input(&world));
}

/// A focused edit box in a mod overlay beside the inventory keeps Escape from closing it, but a
/// click on a slot still reaches the inventory and moves that focus away, as a pointer press
/// does.
#[test]
fn a_focused_overlay_edit_box_gives_way_to_slot_clicks() {
    use mod_ui::mod_screens::ModScreensInput;
    use server_experience::screen;
    let Some(mut presentation) = client_ui::test_support::engine_presentation() else {
        eprintln!("skipping overlay focus click test: missing installed UI carrier; make assets");
        return;
    };
    let (mut player, mut ui) = player_with_stone();
    open_screen(&mut player, &mut ui, None).unwrap();
    flush(&mut player, &mut ui);
    let template = serde_json::json!({"namespace":"focus_test", "overlay": {
        "type":"panel", "size":["100%","100%"], "controls":[{"search": {
            "type":"edit_box", "size":[20,10], "anchor_from":"top_left", "anchor_to":"top_left",
            "text_box_name":"focus_test.search", "max_length":32, "text_control":"display",
            "bindings":[{"binding_name":"#offset"}],
            "button_mappings":[
                {"from_button_id":"button.menu_select","to_button_id":"button.text_edit_box_selected",
                    "handle_select":true,"handle_deselect":false,"mapping_type":"pressed"},
                {"from_button_id":"button.menu_select","to_button_id":"button.text_edit_box_selected",
                    "handle_select":false,"handle_deselect":true,"mapping_type":"global","consume_event":false}],
            "controls":[{"display":{"type":"label","text":"","size":[20,10]}}]}}]}});
    let files = Arc::new(screen::Files {
        namespace: "focus_test".into(),
        templates: [(
            "ui/overlay.json".into(),
            serde_json::to_vec(&template).unwrap(),
        )]
        .into(),
        textures: Vec::new(),
    });
    let overlay = "ui/overlay.json".to_owned();
    let mut data = screen::Modal::default();
    let dpi = ui::DpiScale::new(1.0).unwrap();
    let redraw = |presentation: &mut UiPresentationRuntime, data: &screen::Modal, focus: bool| {
        let focus = focus.then(|| (1, "focus_test.search".to_owned()));
        presentation.set_mod_screens(Some(ModScreensInput {
            id: "focus_test",
            files: &files,
            overlay: Some(&overlay),
            view: None,
            focus: focus.as_ref(),
            data,
        }));
        presentation
            .build(&player, &ui, 0, [1920, 1080], dpi)
            .unwrap();
    };
    redraw(&mut presentation, &data, false);
    let gui = presentation.mod_ui().mod_screen_layout().unwrap().gui;
    data.set_value(
        "#offset".into(),
        screen::Value::Numbers(vec![gui.x - 24.0, gui.y + 4.0]),
    );
    redraw(&mut presentation, &data, true);
    assert!(presentation.mod_ui().mod_text_focused());

    let mut world = World::new();
    world.insert_resource(UiPresentationResource(presentation));
    assert!(inventory_takes_pointer(&world, &player, &ui));
    assert!(!escape_reaches_inventory(&world, &player, &ui));
    let point = world
        .resource::<UiPresentationResource>()
        .0
        .inventory_screen_cells(InventoryScreen::of_runtime(&player, &ui))
        .into_iter()
        .find(|(hit, _)| *hit == InventoryCellHit::Player(9))
        .and_then(|(_, point)| point);
    focus_mod_screens_at(&mut world, point);
    assert!(
        !world
            .resource::<UiPresentationResource>()
            .0
            .mod_ui()
            .mod_text_focused(),
        "the slot click moved focus away from the edit box"
    );
}

/// A server experience's consent prompt blocks the player's inventory input, so the endpoint
/// refuses inventory commands while it is up.
#[test]
fn a_consent_prompt_holds_the_inventory_input() {
    let mut world = World::new();
    assert!(!presentation_holds_input(&world));
    world.insert_resource(client_state::ConsentInput(false));
    assert!(!presentation_holds_input(&world));
    world.insert_resource(client_state::ConsentInput(true));
    assert!(presentation_holds_input(&world));
}
