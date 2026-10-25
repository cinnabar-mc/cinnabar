//! Dragging a stack across the hotbar on the creative screen splits it one newly
//! entered slot at a time; each step must cost work proportional to what it changed,
//! not to the open catalog.

use super::*;
use crate::ui_runtime::inventory_drag::PointerAction;

const STACK: u16 = 64;

/// Fills hotbar slot 0 with a full stack of dirt in the open personal inventory.
fn stock_hotbar(player_runtime: &mut player_state::PlayerState, runtime: &mut UiRuntime) {
    let ledger = runtime.inventory_ledger_mut(player_runtime);
    if !ledger.personal_inventory_desired_open() {
        assert!(ledger.request_personal_open(42));
    }
    let _ = ledger.mark_transport_enqueued(0);
    let registry = protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    };
    ledger.apply_registry(&registry);
    let dirt = registry
        .entries
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:dirt")
        .unwrap()
        .network_id;
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 0, 0],
        runtime_entity_id: -1,
    }));
    let mut slots = vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = NetworkItemStack {
        network_id: dirt,
        stack_network_id: 101,
        count: STACK,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
}

fn count(stack: Option<&NetworkItemStack>) -> u16 {
    stack.map_or(0, |stack| stack.count)
}

// Each slot a split drag enters lays the creative screen out once, placing only
// the changed slots, and allocates under a twentieth of what opening the screen did; the
// result matches a screen laid out from scratch, and the stack divides as
// vanilla's split does: evenly, the remainder staying held.
#[test]
fn split_drag_steps_lay_out_only_the_changed_slots() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping split_drag_steps_lay_out_only_the_changed_slots: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = creative_with(&mut player_runtime, 1_500);
    stock_hotbar(&mut player_runtime, &mut runtime);
    let dpi = DpiScale::new(1.0).unwrap();
    let mut now = 0;
    let mut frame = |presentation: &mut UiPresentationRuntime,
                     player_runtime: &player_state::PlayerState,
                     runtime: &UiRuntime| {
        now += 16;
        let layouts = presentation.engine_container_layouts();
        let placed = presentation.engine_container_placed();
        let (_, allocations) = test_alloc::count(|| {
            presentation
                .build(player_runtime, runtime, now, [1280, 720], dpi)
                .unwrap()
        });
        (
            allocations,
            presentation.engine_container_layouts() - layouts,
            presentation.engine_container_placed() - placed,
        )
    };
    let (opening, _, opening_placed) = frame(&mut presentation, &player_runtime, &runtime);
    frame(&mut presentation, &player_runtime, &runtime);
    runtime.perform_pointer_action(
        &mut player_runtime,
        PointerAction::Click(InventoryCellHit::Player(0)),
    );
    runtime.perform_pointer_action(
        &mut player_runtime,
        PointerAction::Click(InventoryCellHit::Player(1)),
    );
    frame(&mut presentation, &player_runtime, &runtime);
    let mut cells = vec![InventoryCellHit::Player(1)];
    let mut steps = Vec::new();
    for slot in 2..9u8 {
        cells.push(InventoryCellHit::Player(slot));
        runtime.perform_pointer_action(
            &mut player_runtime,
            PointerAction::Distribute {
                cells: cells.clone(),
                one_each: false,
            },
        );
        steps.push(frame(&mut presentation, &player_runtime, &runtime));
        let ledger = player_runtime.inventory.ledger();
        let share = STACK / u16::from(slot);
        for cell in 1..=slot {
            assert_eq!(
                count(ledger.displayed_stack(cell)),
                share,
                "slot {cell} after splitting over {slot} slots"
            );
        }
        assert_eq!(
            count(ledger.cursor_stack()),
            STACK - share * u16::from(slot),
            "the remainder stays held after splitting over {slot} slots"
        );
    }
    for &(allocations, layouts, placed) in &steps {
        assert_eq!(layouts, 1, "{steps:?}");
        assert!(
            placed * 10 < opening_placed,
            "{steps:?} vs {opening_placed}"
        );
        assert!(allocations * 20 < opening, "{steps:?} vs {opening}");
    }

    let Some(mut cold) = engine_presentation_with(super::super::forms::pack_harness::font()) else {
        unreachable!("the fixture loaded above");
    };
    cold.build(&player_runtime, &runtime, now, [1280, 720], dpi)
        .unwrap();
    let warm = presentation.engine_container_frame().unwrap();
    let cold = cold.engine_container_frame().unwrap();
    assert_eq!(*warm.hits, *cold.hits);
    assert_eq!(warm.report, cold.report);
}
