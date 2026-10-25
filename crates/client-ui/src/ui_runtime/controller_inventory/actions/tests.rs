use super::*;
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, InventorySlotEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion,
    SlotIdentity,
};
use std::sync::Arc;

use ContainerControllerAction as Action;
use ui_screens::container_kinds;

const ITEM: i32 = 5;

fn stack(count: u16, stack_network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id: ITEM,
        count,
        stack_network_id,
        ..NetworkItemStack::empty()
    }
}

fn open(window_id: i32, window_type: i8) -> InventoryEvent {
    InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type,
        position: [1, 2, 3],
        runtime_entity_id: -1,
    })
}

/// Server-confirmed contents of the open window's own cells.
fn window_content(window_id: i32, slots: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(window_id),
            slot_type: Some(7),
            dynamic_id: Some(777),
        },
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    })
}

/// Puts `held` on the cursor with a settled server identity.
fn set_cursor(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    held: NetworkItemStack,
) {
    runtime
        .inventory_ledger_mut(player)
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot: 0,
            },
            stack: held,
            storage_item: None,
        }));
}

/// A 27-cell chest: three items in cell 2 and five in cell 3, an empty player inventory,
/// and the fixture item registered with a stack limit of 64.
fn chest() -> (UiRuntime, player_state::PlayerState) {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            identifier: Arc::from("fixture:item"),
            network_id: ITEM,
            component_based: false,
            version: ItemRegistryVersion::None,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    });
    ledger.apply(&open(1, protocol::WINDOW_TYPE_CONTAINER));
    let mut cells = vec![NetworkItemStack::empty(); 27];
    cells[2] = stack(3, 91);
    cells[3] = stack(5, 92);
    ledger.apply(&window_content(1, cells));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: vec![NetworkItemStack::empty(); 36].into(),
        storage_item: NetworkItemStack::empty(),
    }));
    runtime.inventory_open = true;
    (runtime, player)
}

const GEN: u64 = 1;

fn selection(hit: InventoryCellHit, generation: u64) -> Selection {
    Selection {
        generation,
        geometry_revision: 1,
        target: super::super::navigation::Target {
            key: "fixture/target".into(),
            identity: 1,
            hit,
        },
    }
}

/// Dispatches against the screen as it stands now, as a coordinator with a fresh target would.
fn act(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    hit: InventoryCellHit,
    action: Action,
) -> ControllerActionOutcome {
    let token = runtime
        .container_screen_token(player, GEN)
        .expect("the fixture screen is open");
    let selected = selection(hit, GEN);
    runtime.dispatch_container_controller_action(player, action, token, &selected, &selected)
}

/// The output cell the open window's screen table names, so no index is repeated here.
fn furnace_output_hit() -> InventoryCellHit {
    let screen = container_kinds::window_kind(WindowKind::Furnace).unwrap();
    let (_, cells) = screen
        .collections
        .iter()
        .find(|(name, _)| name.ends_with(OUTPUT_COLLECTION_SUFFIX))
        .unwrap();
    cells[0].hit()
}

fn cursor_count(player: &player_state::PlayerState) -> Option<u16> {
    player.inventory.ledger().cursor_stack().map(|s| s.count)
}

fn storage_count(player: &player_state::PlayerState, slot: u8) -> Option<u16> {
    player
        .inventory
        .ledger()
        .storage_stack(slot)
        .map(|s| s.count)
}

fn admitted(outcome: ControllerActionOutcome) -> bool {
    matches!(outcome, ControllerActionOutcome::Admitted { .. })
}

/// A whole-stack take then place moves exactly the stack, one request each.
#[test]
fn primary_takes_the_stack_then_places_it_in_an_empty_cell() {
    let (mut runtime, mut player) = chest();
    let take = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(2),
        Action::Primary,
    );
    assert!(admitted(take), "{take:?}");
    assert_eq!(cursor_count(&player), Some(3));
    assert_eq!(storage_count(&player, 2), None);

    let place = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(5),
        Action::Primary,
    );
    assert!(admitted(place), "{place:?}");
    assert_eq!(storage_count(&player, 5), Some(3));
    assert_eq!(cursor_count(&player), None);
}

/// Half takes the larger half of an odd stack and leaves the rest.
#[test]
fn half_takes_the_larger_half_of_an_odd_stack() {
    let (mut runtime, mut player) = chest();
    let outcome = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(2),
        Action::HalfOrPlaceOne,
    );
    assert!(admitted(outcome), "{outcome:?}");
    assert_eq!(cursor_count(&player), Some(2));
    assert_eq!(storage_count(&player, 2), Some(1));
}

/// With a carried stack the same action places a single item.
#[test]
fn half_places_one_item_from_the_carried_stack() {
    let (mut runtime, mut player) = chest();
    set_cursor(&mut runtime, &mut player, stack(4, 77));
    let outcome = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(6),
        Action::HalfOrPlaceOne,
    );
    assert!(admitted(outcome), "{outcome:?}");
    assert_eq!(storage_count(&player, 6), Some(1));
    assert_eq!(cursor_count(&player), Some(3));
}

/// Quick transfer queues exactly one request and clears the source prediction.
#[test]
fn quick_transfer_admits_one_request() {
    let (mut runtime, mut player) = chest();
    let outcome = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(2),
        Action::QuickTransfer,
    );
    assert!(admitted(outcome), "{outcome:?}");
    assert_eq!(player.inventory.ledger().pending_request_count(), 1);
}

/// Gather stops at the stack limit instead of overfilling the carried stack.
#[test]
fn gather_fills_the_carried_stack_up_to_its_limit() {
    let (mut runtime, mut player) = chest();
    set_cursor(&mut runtime, &mut player, stack(62, 77));
    let outcome = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(2),
        Action::Gather,
    );
    assert!(admitted(outcome), "{outcome:?}");
    assert_eq!(cursor_count(&player), Some(64));
}

/// Gather needs a carried stack and refuses before touching the ledger.
#[test]
fn gather_without_a_carried_stack_is_not_applicable() {
    let (mut runtime, mut player) = chest();
    let outcome = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(2),
        Action::Gather,
    );
    assert_eq!(
        outcome,
        ControllerActionOutcome::NotApplicable(ActionUnavailable::CursorEmpty)
    );
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}

/// Drop is explicit: one or all of the carried stack, and never from an empty cell.
#[test]
fn drop_acts_on_the_carried_stack_only_when_asked() {
    let (mut runtime, mut player) = chest();
    set_cursor(&mut runtime, &mut player, stack(4, 77));
    let one = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(9),
        Action::DropOne,
    );
    assert!(admitted(one), "{one:?}");
    assert_eq!(cursor_count(&player), Some(3));

    let (mut runtime, mut player) = chest();
    set_cursor(&mut runtime, &mut player, stack(4, 77));
    let all = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(9),
        Action::DropAll,
    );
    assert!(admitted(all), "{all:?}");
    assert_eq!(cursor_count(&player), None);

    let (mut runtime, mut player) = chest();
    let empty = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(9),
        Action::DropAll,
    );
    assert_eq!(
        empty,
        ControllerActionOutcome::NotApplicable(ActionUnavailable::NothingToAct)
    );
}

/// A focused text field blocks drop without consuming anything.
#[test]
fn drop_is_blocked_while_a_text_field_owns_input() {
    let (mut runtime, mut player) = chest();
    set_cursor(&mut runtime, &mut player, stack(4, 77));
    runtime.screen_state_mut().anvil_focused = true;
    let outcome = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(9),
        Action::DropOne,
    );
    assert_eq!(
        outcome,
        ControllerActionOutcome::NotApplicable(ActionUnavailable::TextEntry)
    );
    assert_eq!(cursor_count(&player), Some(4));
}

/// When admission refuses, what was admitted stays consistent and nothing is retried.
#[test]
fn refused_admission_is_reported_and_leaves_the_queue_consistent() {
    let (mut runtime, mut player) = chest();
    set_cursor(&mut runtime, &mut player, stack(500, 77));
    let mut accepted = 0;
    let mut refusal = None;
    for _ in 0..=inventory::inventory_ledger::MAX_PENDING_REQUESTS {
        match act(
            &mut runtime,
            &mut player,
            InventoryCellHit::Storage(9),
            Action::DropOne,
        ) {
            ControllerActionOutcome::Admitted { .. } => accepted += 1,
            ControllerActionOutcome::Rejected(error) => {
                refusal = Some(error);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(refusal.is_some(), "the queue never refused");
    assert_eq!(player.inventory.ledger().pending_request_count(), accepted);
    let after = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::Storage(9),
        Action::DropOne,
    );
    assert!(matches!(after, ControllerActionOutcome::Rejected(_)));
    assert_eq!(player.inventory.ledger().pending_request_count(), accepted);
}

/// A target resolved against a replaced server window is refused without mutating anything.
#[test]
fn a_token_from_an_earlier_window_is_stale() {
    let (mut runtime, mut player) = chest();
    let token = runtime.container_screen_token(&player, GEN).unwrap();
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&open(2, protocol::WINDOW_TYPE_CONTAINER));
    let mut cells = vec![NetworkItemStack::empty(); 27];
    cells[2] = stack(3, 191);
    ledger.apply(&window_content(2, cells));

    let outcome = runtime.dispatch_container_controller_action(
        &mut player,
        Action::Primary,
        token,
        &selection(InventoryCellHit::Storage(2), GEN),
        &selection(InventoryCellHit::Storage(2), GEN),
    );
    assert_eq!(
        outcome,
        ControllerActionOutcome::NotApplicable(ActionUnavailable::StaleWindow)
    );
    assert_eq!(cursor_count(&player), None);
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}

/// A personal inventory carries no window facts, so only the caller's generation can tell a
/// reopened screen from the one a target was resolved against.
#[test]
fn a_new_screen_generation_makes_a_personal_token_stale() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    runtime.inventory_open = true;
    let first = runtime.container_screen_token(&player, 1).unwrap();
    assert_eq!(
        first,
        runtime.container_screen_token(&player, 1).unwrap(),
        "the same opening yields the same token"
    );
    assert_ne!(first, runtime.container_screen_token(&player, 2).unwrap());

    let outcome = runtime.dispatch_container_controller_action(
        &mut player,
        Action::Primary,
        first,
        &selection(InventoryCellHit::Player(0), 1),
        &selection(InventoryCellHit::Player(0), 2),
    );
    assert_eq!(
        outcome,
        ControllerActionOutcome::NotApplicable(ActionUnavailable::StaleWindow)
    );
}

/// A server window the ledger still retains does not count as an open screen once the UI closed.
#[test]
fn retained_server_window_with_the_ui_closed_is_closed() {
    let (mut runtime, mut player) = chest();
    let token = runtime.container_screen_token(&player, GEN).unwrap();
    runtime.inventory_open = false;
    assert!(player.inventory.ledger().window_kind().is_some());
    assert_eq!(runtime.container_screen_token(&player, GEN), None);
    assert!(
        runtime
            .container_controller_hints(&player, InventoryCellHit::Storage(2))
            .is_empty()
    );
    for action in Action::ALL {
        let outcome = runtime.dispatch_container_controller_action(
            &mut player,
            action,
            token,
            &selection(InventoryCellHit::Storage(2), GEN),
            &selection(InventoryCellHit::Storage(2), GEN),
        );
        assert_eq!(
            outcome,
            ControllerActionOutcome::NotApplicable(ActionUnavailable::ScreenClosed)
        );
    }
    assert_eq!(cursor_count(&player), None);
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}

/// With no container screen every action is unavailable and nothing is offered.
#[test]
fn closed_screen_offers_nothing() {
    let (mut runtime, player) = chest();
    runtime.inventory_open = false;
    let hit = InventoryCellHit::Player(0);
    assert!(runtime.container_controller_hints(&player, hit).is_empty());
    assert!(
        runtime
            .container_controller_capabilities(&player, hit)
            .iter()
            .all(|c| c.availability == Err(ActionUnavailable::ScreenClosed))
    );
}

/// The furnace output only gives items out, so nothing can be placed into it.
#[test]
fn furnace_output_refuses_placement_without_touching_the_ledger() {
    let (mut runtime, mut player) = chest();
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&open(2, protocol::WINDOW_TYPE_FURNACE));
    set_cursor(&mut runtime, &mut player, stack(4, 77));
    let outcome = act(
        &mut runtime,
        &mut player,
        furnace_output_hit(),
        Action::Primary,
    );
    assert_eq!(
        outcome,
        ControllerActionOutcome::NotApplicable(ActionUnavailable::OutputOnly)
    );
    assert_eq!(cursor_count(&player), Some(4));
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}

/// Hint eligibility is not admission: an eligible action can still be refused at dispatch,
/// and then nothing is published.
#[test]
fn an_offered_action_can_still_be_refused_by_the_ledger() {
    let (mut runtime, mut player) = chest();
    let hit = InventoryCellHit::CraftOutput;
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&open(2, protocol::WINDOW_TYPE_WORKBENCH));
    let offered = runtime.container_controller_hints(&player, hit);
    assert!(offered.iter().any(|h| h.action == Action::Primary));
    let outcome = act(&mut runtime, &mut player, hit, Action::Primary);
    assert!(
        matches!(outcome, ControllerActionOutcome::Rejected(_)),
        "{outcome:?}"
    );
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}

/// An occupied target under a carried stack is a generic move: merge versus swap is the
/// ledger's call, so the prompt does not guess.
#[test]
fn carried_stack_over_an_occupied_cell_is_a_generic_move() {
    let (mut runtime, mut player) = chest();
    set_cursor(&mut runtime, &mut player, stack(4, 77));
    let primary = runtime
        .container_controller_hints(&player, InventoryCellHit::Storage(2))
        .into_iter()
        .find(|h| h.action == Action::Primary)
        .unwrap();
    assert_eq!(primary.intent, ActionIntent::Move);
}

/// Output cells claim only the actions with a stated meaning; the rest stay unresolved.
#[test]
fn craft_output_claims_only_primary_and_craft_all() {
    let (mut runtime, mut player) = chest();
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&open(2, protocol::WINDOW_TYPE_WORKBENCH));
    let caps = runtime.container_controller_capabilities(&player, InventoryCellHit::CraftOutput);
    let availability = |action| {
        caps.iter()
            .find(|c| c.action == action)
            .map(|c| c.availability)
            .unwrap()
    };
    assert_eq!(availability(Action::Primary), Ok(ActionIntent::CraftOnce));
    assert_eq!(
        availability(Action::QuickTransfer),
        Ok(ActionIntent::CraftAll)
    );
    assert_eq!(
        availability(Action::HalfOrPlaceOne),
        Err(ActionUnavailable::UnresolvedMapping)
    );
    assert_eq!(
        availability(Action::Gather),
        Err(ActionUnavailable::NotApplicable)
    );
}

/// A craft the ledger cannot form is a refusal that leaves cursor and queue alone.
#[test]
fn craft_with_an_unformed_recipe_is_rejected_without_mutation() {
    let (mut runtime, mut player) = chest();
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&open(2, protocol::WINDOW_TYPE_WORKBENCH));
    let outcome = act(
        &mut runtime,
        &mut player,
        InventoryCellHit::CraftOutput,
        Action::Primary,
    );
    assert!(
        matches!(outcome, ControllerActionOutcome::Rejected(_)),
        "{outcome:?}"
    );
    assert_eq!(cursor_count(&player), None);
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}

/// Prompts are the available capabilities, in prompt order, with context-aware wording.
#[test]
fn hints_follow_the_target_and_the_carried_stack() {
    let (mut runtime, mut player) = chest();
    let intents = |runtime: &UiRuntime, player: &player_state::PlayerState, hit| {
        runtime
            .container_controller_hints(player, hit)
            .into_iter()
            .map(|hint| hint.intent)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        intents(&runtime, &player, InventoryCellHit::Storage(2)),
        [
            ActionIntent::Take,
            ActionIntent::TakeHalf,
            ActionIntent::QuickTransfer,
            ActionIntent::DropOne,
            ActionIntent::DropAll,
        ]
    );
    assert!(intents(&runtime, &player, InventoryCellHit::Storage(9)).is_empty());

    set_cursor(&mut runtime, &mut player, stack(4, 77));
    assert_eq!(
        intents(&runtime, &player, InventoryCellHit::Storage(2)),
        [
            ActionIntent::Move,
            ActionIntent::PlaceOne,
            ActionIntent::QuickTransfer,
            ActionIntent::Gather,
            ActionIntent::DropOne,
            ActionIntent::DropAll,
        ]
    );
}

/// No capability declares hold or repeat behavior, and all defer admission to dispatch.
#[test]
fn capabilities_are_press_only_and_revalidated() {
    let (runtime, player) = chest();
    let caps = runtime.container_controller_capabilities(&player, InventoryCellHit::Storage(2));
    assert_eq!(caps.len(), Action::ALL.len());
    assert!(
        caps.iter()
            .all(|c| c.trigger == Trigger::Press && c.admission == Admission::AtDispatch)
    );
}

#[test]
fn replacing_a_filtered_row_cannot_activate_its_new_contents() {
    let (mut runtime, mut player) = chest();
    let screen = runtime.container_screen_token(&player, GEN).unwrap();
    let resolved = selection(InventoryCellHit::RecipeBook(0), GEN);
    let mut current = resolved.clone();
    current.target.identity += 1;
    let outcome = runtime.dispatch_container_controller_action(
        &mut player,
        Action::Primary,
        screen,
        &resolved,
        &current,
    );
    assert_eq!(
        outcome,
        ControllerActionOutcome::NotApplicable(ActionUnavailable::StaleTarget)
    );
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}

#[test]
fn layout_replacement_invalidates_a_resolved_target() {
    let (mut runtime, mut player) = chest();
    let screen = runtime.container_screen_token(&player, GEN).unwrap();
    let resolved = selection(InventoryCellHit::Storage(2), GEN);
    let mut current = resolved.clone();
    current.geometry_revision += 1;
    assert_eq!(
        runtime.dispatch_container_controller_action(
            &mut player,
            Action::Primary,
            screen,
            &resolved,
            &current,
        ),
        ControllerActionOutcome::NotApplicable(ActionUnavailable::StaleTarget),
    );
    assert_eq!(cursor_count(&player), None);
}

#[test]
fn non_crafting_station_results_do_not_claim_craft_all() {
    let (mut runtime, mut player) = chest();
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&open(2, protocol::WINDOW_TYPE_ANVIL));
    let hints = runtime.container_controller_hints(&player, InventoryCellHit::CraftOutput);
    assert!(hints.contains(&ActionHint {
        action: Action::Primary,
        intent: ActionIntent::TakeOutput
    }));
    assert!(hints.contains(&ActionHint {
        action: Action::QuickTransfer,
        intent: ActionIntent::QuickTransfer
    }));
    assert!(!hints.iter().any(|hint| matches!(
        hint.intent,
        ActionIntent::CraftOnce | ActionIntent::CraftAll
    )));
}
