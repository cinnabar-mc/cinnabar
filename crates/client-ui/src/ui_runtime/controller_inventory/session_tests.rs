use super::*;
use json_ui::{FocusMeta, HitKind, HitRegion, RectOut};
use std::time::Duration;

fn session() -> ControllerSession {
    ControllerSession::new(navigation::Settings {
        cursor_units_per_second: 100.0,
        cursor_deadzone: 0.2,
        repeat_delay: Duration::from_millis(300),
        repeat_interval: Duration::from_millis(100),
    })
    .unwrap()
}

fn bounds() -> RectOut {
    RectOut {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    }
}

fn region() -> HitRegion {
    HitRegion {
        key: "cell".into(),
        name: "cell".into(),
        kind: HitKind::Button,
        rect: RectOut {
            x: 10.0,
            y: 10.0,
            w: 10.0,
            h: 10.0,
        },
        clip: bounds(),
        layer: 0,
        order: 0,
        pressed: None,
        control_name: None,
        collection_index: None,
        collection: None,
        enabled: true,
        checked: None,
        max_length: None,
        group_index: None,
        renderer: None,
        drag_axes: [false; 2],
        sound: None,
        input: Default::default(),
        focus: Some(FocusMeta {
            enabled: true,
            precedence: 1,
            wrap: false,
            magnet: false,
            identifier: "cell".into(),
            reset_on_focus_lost: true,
            change: Default::default(),
            mapping: Vec::new(),
            containers: Vec::new(),
        }),
        collections: Vec::new(),
        widget: Default::default(),
        modal_root: None,
        transform: None,
    }
}

fn input(time: u64, actions: bool) -> Input {
    Input {
        navigation: navigation::Input {
            now: Duration::from_millis(time),
            controller: 1,
            active: true,
            directions: [false; 4],
            axes: [0.0; 2],
            mouse: None,
        },
        actions: [actions; ContainerControllerAction::ALL.len()],
    }
}

fn step(session: &mut ControllerSession, runtime: &mut UiRuntime, input: Input) -> Output {
    let targets = session.targets([("cell".into(), InventoryCellHit::Player(0))]);
    let hits = [region()];
    let generation = runtime.container_generation();
    session.step(
        runtime,
        navigation::Frame {
            generation,
            geometry_revision: 1,
            bounds: bounds(),
            hits: &hits,
            targets: &targets,
        },
        input,
    )
}

fn focus(session: &mut ControllerSession, runtime: &mut UiRuntime, time: u64) {
    let mut sample = input(time, false);
    sample.navigation.directions[3] = true;
    assert!(
        step(session, runtime, sample)
            .navigation
            .selection
            .is_some()
    );
}

#[test]
fn generation_records_same_frame_close_reopen_and_session_replacement() {
    let mut runtime = UiRuntime::new(1);
    runtime.set_inventory_open(true);
    let first = runtime.container_generation();
    runtime.set_inventory_open(false);
    runtime.set_inventory_open(true);
    assert!(runtime.container_generation() > first);
    let reopened = runtime.container_generation();
    runtime.begin_session(2);
    assert!(runtime.container_generation() > reopened);
}

#[test]
fn interned_cells_survive_reflow_and_duplicate_regions_are_single_targets() {
    let mut session = session();
    let first = session.targets([("old".into(), InventoryCellHit::Storage(4))]);
    let next = session.targets([
        ("new".into(), InventoryCellHit::Storage(4)),
        ("duplicate".into(), InventoryCellHit::Storage(4)),
        ("recipe".into(), InventoryCellHit::RecipeBook(0)),
        ("creative".into(), InventoryCellHit::CreativeGrid(0)),
    ]);
    assert_eq!(next.len(), 1);
    assert_eq!(first[0].identity, next[0].identity);
    assert_eq!(next[0].key, "new");
}

#[test]
fn all_actions_require_full_neutral_across_open_modal_disconnect_and_controller_change() {
    let mut session = session();
    let mut runtime = UiRuntime::new(1);
    runtime.set_inventory_open(true);
    assert!(
        step(&mut session, &mut runtime, input(0, true))
            .actions
            .is_empty()
    );
    step(&mut session, &mut runtime, input(1, false));
    focus(&mut session, &mut runtime, 2);
    assert_eq!(
        step(&mut session, &mut runtime, input(3, true)).actions,
        ContainerControllerAction::ALL
    );
    assert!(
        step(&mut session, &mut runtime, input(4, true))
            .actions
            .is_empty()
    );
    for interruption in 0..3 {
        let mut sample = input(5 + interruption * 5, true);
        if interruption < 2 {
            sample.navigation.active = false;
        } else {
            sample.navigation.controller = 2;
        }
        assert!(step(&mut session, &mut runtime, sample).actions.is_empty());
        assert!(
            step(
                &mut session,
                &mut runtime,
                input(6 + interruption * 5, true)
            )
            .actions
            .is_empty()
        );
        step(
            &mut session,
            &mut runtime,
            input(7 + interruption * 5, false),
        );
        focus(&mut session, &mut runtime, 8 + interruption * 5);
        assert_eq!(
            step(
                &mut session,
                &mut runtime,
                input(9 + interruption * 5, true)
            )
            .actions
            .len(),
            ContainerControllerAction::ALL.len()
        );
    }
}

#[test]
fn mouse_takeover_and_blank_space_never_emit_actions() {
    let mut session = session();
    let mut runtime = UiRuntime::new(1);
    runtime.set_inventory_open(true);
    step(&mut session, &mut runtime, input(0, false));
    focus(&mut session, &mut runtime, 1);
    assert!(runtime.inventory_controller_cursor_visible());
    let mut mouse = input(2, true);
    mouse.navigation.mouse = Some([90.0, 90.0]);
    let output = step(&mut session, &mut runtime, mouse);
    assert!(output.navigation.selection.is_none());
    assert!(output.actions.is_empty());
    assert!(!runtime.inventory_controller_cursor_visible());
    assert!(runtime.inventory_controller_focus().is_none());
    assert!(
        step(&mut session, &mut runtime, input(3, true))
            .actions
            .is_empty()
    );
    step(&mut session, &mut runtime, input(4, false));
    assert!(
        step(&mut session, &mut runtime, input(5, true))
            .actions
            .is_empty()
    );
}

#[test]
fn stale_recipe_rows_never_become_action_selections() {
    let mut session = session();
    assert!(
        session
            .targets([("row".into(), InventoryCellHit::RecipeBook(0))])
            .is_empty()
    );
    assert!(
        session
            .targets([("row".into(), InventoryCellHit::RecipeBook(1))])
            .is_empty()
    );
}

#[test]
fn text_entry_owns_input_even_if_caller_marks_container_active() {
    let mut session = session();
    let mut runtime = UiRuntime::new(1);
    runtime.set_inventory_open(true);
    step(&mut session, &mut runtime, input(0, false));
    focus(&mut session, &mut runtime, 1);
    runtime.screen_state_mut().search_focused = true;
    assert!(
        step(&mut session, &mut runtime, input(2, true))
            .actions
            .is_empty()
    );
    assert!(runtime.screen_state().search_focused);
    assert!(runtime.inventory_controller_focus().is_none());
    runtime.screen_state_mut().search_focused = false;
    assert!(
        step(&mut session, &mut runtime, input(3, true))
            .actions
            .is_empty()
    );
}

#[test]
fn action_only_mouse_handoff_keeps_controller_cursor_on_next_neutral_sample() {
    let mut session = session();
    let mut runtime = UiRuntime::new(1);
    runtime.set_inventory_open(true);
    let mut mouse = input(0, false);
    mouse.navigation.mouse = Some([15.0, 15.0]);
    step(&mut session, &mut runtime, mouse);
    assert!(!runtime.inventory_controller_cursor_visible());
    step(&mut session, &mut runtime, input(1, false));
    let action = step(&mut session, &mut runtime, input(2, true));
    assert_eq!(action.navigation.mode, navigation::Mode::Cursor);
    assert_eq!(action.navigation.cursor, Some([15.0, 15.0]));
    assert!(runtime.inventory_controller_cursor_visible());
    let neutral = step(&mut session, &mut runtime, input(3, false));
    assert_eq!(neutral.navigation.mode, navigation::Mode::Cursor);
    assert_eq!(neutral.navigation.selection, action.navigation.selection);
    assert!(runtime.inventory_controller_cursor_visible());
}

#[test]
fn unsupported_focusable_recipe_cannot_trap_ordinary_cell_navigation() {
    let mut session = session();
    let mut runtime = UiRuntime::new(1);
    runtime.set_inventory_open(true);
    let mut hits = [region(), region(), region()];
    for (index, hit) in hits.iter_mut().enumerate() {
        hit.key = format!("cell{index}");
        hit.focus.as_mut().unwrap().identifier = hit.key.clone();
        hit.rect.x += index as f64 * 20.0;
        hit.order = index;
    }
    let targets = session.targets([
        (hits[0].key.clone(), InventoryCellHit::Player(0)),
        (hits[1].key.clone(), InventoryCellHit::RecipeBook(0)),
        (hits[2].key.clone(), InventoryCellHit::Player(1)),
    ]);
    let sample = |session: &mut ControllerSession, runtime: &mut UiRuntime, input| {
        let generation = runtime.container_generation();
        session.step(
            runtime,
            navigation::Frame {
                generation,
                geometry_revision: 1,
                bounds: bounds(),
                hits: &hits,
                targets: &targets,
            },
            input,
        )
    };
    sample(&mut session, &mut runtime, input(0, false));
    let mut right = input(1, false);
    right.navigation.directions[3] = true;
    let first = sample(&mut session, &mut runtime, right);
    assert_eq!(
        first.navigation.selection.unwrap().target.hit,
        InventoryCellHit::Player(0)
    );
    sample(&mut session, &mut runtime, input(2, false));
    let mut right = input(3, false);
    right.navigation.directions[3] = true;
    let second = sample(&mut session, &mut runtime, right);
    assert_eq!(
        second.navigation.selection.unwrap().target.hit,
        InventoryCellHit::Player(1)
    );
}

#[test]
fn snapshot_restores_focus_cursor_pointer_and_generation() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    runtime.set_inventory_open(true);
    runtime.set_inventory_controller_focus(Some("before".into()));
    runtime.set_inventory_controller_cursor_visible(true);
    runtime.set_inventory_pointer_gui(Some([15.0, 15.0]));
    let generation = runtime.container_generation();
    let captured = runtime.capture_presentation_inventory(&player);
    runtime.close_inventory(&mut player);
    runtime.set_inventory_controller_focus(Some("after".into()));
    runtime.set_inventory_pointer_gui(Some([40.0, 50.0]));
    let after_generation = runtime.container_generation();
    runtime.with_presentation_inventory(&player, captured, |runtime, _| {
        assert_eq!(runtime.inventory_controller_focus(), Some("before"));
        assert!(runtime.inventory_controller_cursor_visible());
        assert_eq!(runtime.inventory_pointer_gui(), Some([15.0, 15.0]));
        assert_eq!(runtime.container_generation(), generation);
    });
    assert_eq!(runtime.inventory_controller_focus(), Some("after"));
    assert!(!runtime.inventory_controller_cursor_visible());
    assert_eq!(runtime.inventory_pointer_gui(), Some([40.0, 50.0]));
    assert_eq!(runtime.container_generation(), after_generation);
}

#[test]
fn public_close_and_reopen_advance_generation_without_a_session_step() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    runtime
        .publish_local_runtime_id(&mut player, 1, 42)
        .unwrap();
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
    runtime.toggle_inventory(&mut player, None);
    assert!(runtime.inventory_open());
    let first = runtime.container_generation();
    runtime.close_inventory(&mut player);
    assert!(!runtime.inventory_open());
    let closed = runtime.container_generation();
    assert!(closed > first);
    runtime.toggle_inventory(&mut player, None);
    assert!(runtime.inventory_open());
    assert!(runtime.container_generation() > closed);
}

#[test]
fn unsupported_modal_still_blocks_underlying_ordinary_cells() {
    let mut session = session();
    let mut runtime = UiRuntime::new(1);
    runtime.set_inventory_open(true);
    let mut modal = region();
    modal.key = "modal".into();
    modal.kind = HitKind::Modal;
    let hits = [region(), modal];
    let targets = session.targets([("cell".into(), InventoryCellHit::Player(0))]);
    for time in 0..2 {
        let mut input = input(time, false);
        input.navigation.directions[3] = time == 1;
        let generation = runtime.container_generation();
        let output = session.step(
            &mut runtime,
            navigation::Frame {
                generation,
                geometry_revision: 1,
                bounds: bounds(),
                hits: &hits,
                targets: &targets,
            },
            input,
        );
        assert!(output.navigation.selection.is_none());
        assert!(output.actions.is_empty());
    }
}
