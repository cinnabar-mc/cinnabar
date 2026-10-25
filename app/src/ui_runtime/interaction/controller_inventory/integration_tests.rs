//! Production input-owner regressions with a rendered container and Bevy gamepad state.

use super::*;
use bevy::{
    ecs::message::Messages,
    input::{
        ButtonInput, ButtonState,
        gamepad::{Gamepad, GamepadAxis, GamepadButton},
        keyboard::KeyboardInput,
        mouse::{AccumulatedMouseMotion, MouseButtonInput},
        touch::Touches,
    },
    prelude::{App, Entity, IntoScheduleConfigs, MouseButton, Update},
    time::{Real, Time},
    window::{CursorOptions, PrimaryWindow, Window, WindowEvent, WindowResolution},
};
use client_shell::{
    menu::{MenuClipboard, MenuRuntime},
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_state::{PlayerRuntime, SemanticInputSnapshot};
use client_ui::{
    test_support::{engine_presentation, inventory_session},
    ui_runtime::presentation::UiPresentationRuntime,
};
use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack};
use semantic_input::Action;
use std::sync::Arc;
use ui::{DpiScale, UiPoint};
use ui_screens::container_actions::InventoryCellHit;

const VIEWPORT: [u32; 2] = [1280, 720];

struct Harness {
    app: App,
    window: Entity,
    pad: Entity,
}

/// Fills every player slot; navigation must still select an occupied player cell.
fn harness(name: &str, open: bool) -> Option<Harness> {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let slots = (0..inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT)
        .map(|slot| NetworkItemStack {
            network_id: 6,
            count: 2,
            stack_network_id: 55 + i32::try_from(slot).expect("inventory slot fits i32"),
            ..NetworkItemStack::default()
        })
        .collect::<Vec<_>>();
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from(slots),
            storage_item: NetworkItemStack::empty(),
        }));
    runtime.toggle_inventory(&mut player, None);
    assert!(runtime.inventory_open());
    assert!(
        runtime
            .inventory_ledger_mut(&mut player)
            .mark_transport_enqueued(0)
    );
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: -1,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));

    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping {name}: missing local engine UI carrier fixture; run `make assets`");
        return None;
    };
    for now in [0, 500, 5_000] {
        presentation
            .build(
                &player,
                &runtime,
                now,
                VIEWPORT,
                DpiScale::new(1.0).unwrap(),
            )
            .expect("render the inventory fixture");
    }
    assert!(
        presentation.engine_container_frame().is_some(),
        "the engine fixture must render a container frame"
    );
    assert!(owns_open_container(&runtime, Some(&presentation)));
    let frame = presentation.engine_container_frame().unwrap();
    assert!(
        json_ui::focus_order(&frame.hits).iter().any(|region| {
            matches!(
                presentation.engine_container_region_hit(region),
                Some(InventoryCellHit::Player(_))
            )
        }),
        "rendered player cells must admit authored focus: {:?}",
        frame
            .hits
            .iter()
            .filter(|region| {
                matches!(
                    presentation.engine_container_region_hit(region),
                    Some(InventoryCellHit::Player(_))
                )
            })
            .take(6)
            .map(|region| (
                &region.key,
                region.enabled,
                region.focus.as_ref().map(|focus| focus.enabled)
            ))
            .collect::<Vec<_>>()
    );
    if !open {
        let _ = runtime.close_inventory(&mut player);
        assert!(!runtime.inventory_open());
    }

    let mut window = Window {
        focused: true,
        resolution: WindowResolution::new(VIEWPORT[0], VIEWPORT[1]),
        ..Default::default()
    };
    window.set_cursor_position(Some(bevy::math::Vec2::new(1.0, 1.0)));

    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<Messages<WindowEvent>>()
        .init_resource::<ButtonInput<bevy::prelude::KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<crate::semantic_controls::SemanticTouchTargets>()
        .init_resource::<crate::semantic_controls::PendingDeviceFrame>()
        .init_resource::<crate::semantic_controls::SemanticRouteState>()
        .init_resource::<crate::semantic_controls::SemanticInputRuntime>()
        .init_resource::<SemanticInputSnapshot>()
        .insert_resource(Time::<Real>::default())
        .insert_resource(PlayerRuntime(player))
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .insert_resource(ControllerInventoryInput::default())
        .insert_resource(MenuClipboard::default())
        .insert_resource(MenuRuntime::new(false, 2, "Tester".to_owned()))
        .add_systems(
            Update,
            (
                crate::semantic_controls::collect_raw_input,
                crate::semantic_controls::route_semantic_input,
                sample_controller_pointer_ownership,
                crate::ui_runtime::interaction::drive_chat_keyboard_input,
                drive_controller_inventory,
                crate::ui_runtime::interaction::drive_inventory_ui_actions,
                crate::ui_runtime::interaction::inventory::drive_world_inventory_keys,
                crate::semantic_controls::finalize_semantic_input_after_ui_authority,
            )
                .chain(),
        );
    let window = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    let pad = app.world_mut().spawn(Gamepad::default()).id();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    Some(Harness { app, window, pad })
}

fn update_neutral(harness: &mut Harness) {
    harness.app.update();
}

fn set_pad(harness: &mut Harness, buttons: &[GamepadButton], left_y: f32) {
    let mut pad = harness
        .app
        .world_mut()
        .get_mut::<Gamepad>(harness.pad)
        .expect("test gamepad");
    pad.digital_mut().reset_all();
    for button in buttons {
        pad.digital_mut().press(*button);
    }
    pad.analog_mut().set(GamepadAxis::LeftStickY, left_y);
}

fn request_count(harness: &Harness) -> usize {
    harness
        .app
        .world()
        .resource::<PlayerRuntime>()
        .inventory
        .ledger()
        .pending_request_count()
}

fn pointer_hit(presentation: &UiPresentationRuntime, target: InventoryCellHit) -> [f32; 2] {
    let frame = presentation.engine_container_frame().unwrap();
    frame
        .hits
        .iter()
        .find_map(|region| {
            if presentation.engine_container_region_hit(region) != Some(target) {
                return None;
            }
            let center = [
                region.rect.x + region.rect.w / 2.0,
                region.rect.y + region.rect.h / 2.0,
            ];
            let gui = region
                .transform
                .map_or(center, |transform| transform.apply(center));
            let gui = gui.map(|axis| axis as f32);
            (presentation.engine_container_hit(gui) == Some(target)).then(|| {
                [
                    frame.origin[0] + gui[0] * frame.scale,
                    frame.origin[1] + gui[1] * frame.scale,
                ]
            })
        })
        .unwrap_or_else(|| panic!("{target:?} has no physical hit point"))
}

/// Moves the virtual cursor to a rendered cell using only normalized stick input and caller time.
fn move_stick_to_cell(harness: &mut Harness, target: InventoryCellHit) {
    let presentation = &harness.app.world().resource::<UiPresentationResource>().0;
    let physical = pointer_hit(presentation, target);
    let desired = presentation
        .inventory_gui_point(
            UiPoint::new(physical[0], physical[1]).unwrap(),
            VIEWPORT,
            1.0,
        )
        .unwrap();
    let bounds = presentation.inventory_gui_bounds(VIEWPORT, 1.0).unwrap();
    let delta = [
        f64::from(desired[0]) - bounds.w / 2.0,
        f64::from(desired[1]) - bounds.h / 2.0,
    ];
    let distance = delta[0].hypot(delta[1]);
    assert!(distance > 0.0);
    set_pad(harness, &[], (-delta[1] / distance) as f32);
    harness
        .app
        .world_mut()
        .get_mut::<Gamepad>(harness.pad)
        .unwrap()
        .analog_mut()
        .set(GamepadAxis::LeftStickX, (delta[0] / distance) as f32);
    harness
        .app
        .world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs_f64(
            distance / SESSION_SETTINGS.cursor_units_per_second,
        ));
    harness.app.update();
    let runtime = harness.app.world().resource::<UiRuntimeResource>();
    let point = runtime
        .inventory_pointer_gui()
        .expect("stick publishes its GUI pointer");
    assert!(runtime.inventory_controller_cursor_visible());
    assert_eq!(
        harness
            .app
            .world()
            .resource::<UiPresentationResource>()
            .0
            .engine_container_hit(point),
        Some(target),
    );
    set_pad(harness, &[], 0.0);
    harness
        .app
        .world_mut()
        .get_mut::<Gamepad>(harness.pad)
        .unwrap()
        .analog_mut()
        .set(GamepadAxis::LeftStickX, 0.0);
    harness.app.update();
}

#[test]
fn stick_cursor_primary_and_quick_transfer_act_on_the_rendered_item_cell() {
    for button in [GamepadButton::South, GamepadButton::North] {
        let Some(mut harness) = harness(
            "stick_cursor_primary_and_quick_transfer_act_on_the_rendered_item_cell",
            true,
        ) else {
            return;
        };
        // An empty transfer destination avoids needing the installed item-limit catalog.
        let mut player = harness.app.world_mut().resource_mut::<PlayerRuntime>();
        let source = player
            .inventory
            .ledger()
            .displayed_stack(0)
            .unwrap()
            .clone();
        let mut slots = vec![
            NetworkItemStack::empty();
            inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT as usize
        ];
        slots[0] = source;
        player
            .inventory
            .ledger_mut()
            .apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }));
        drop(player);
        update_neutral(&mut harness);
        move_stick_to_cell(&mut harness, InventoryCellHit::Player(0));
        let before = request_count(&harness);
        set_pad(&mut harness, &[button], 0.0);
        harness.app.update();
        assert_eq!(
            request_count(&harness),
            before + 1,
            "{button:?} acts on stick selection"
        );
        harness.app.update();
        assert_eq!(
            request_count(&harness),
            before + 1,
            "held {button:?} does not repeat"
        );
    }
}

fn focused_hit(harness: &Harness) -> Option<InventoryCellHit> {
    let presentation = &harness.app.world().resource::<UiPresentationResource>().0;
    let runtime = &harness.app.world().resource::<UiRuntimeResource>();
    let key = runtime.inventory_controller_focus()?;
    let region = presentation
        .engine_container_frame()?
        .hits
        .iter()
        .find(|region| region.key == key)?;
    presentation.engine_container_region_hit(region)
}

/// D-pad focus may begin on empty armor or crafting controls; stop on an occupied player cell.
fn navigate_to_player_cell(harness: &mut Harness) -> InventoryCellHit {
    let focusable_count = harness
        .app
        .world()
        .resource::<UiPresentationResource>()
        .0
        .engine_container_frame()
        .unwrap()
        .hits
        .iter()
        .filter(|hit| hit.takes_focus())
        .count()
        .max(1);
    let mut observed = Vec::new();
    for step in 0..focusable_count {
        set_pad(harness, &[GamepadButton::DPadDown], 0.0);
        harness.app.update();
        let hit = focused_hit(harness);
        if step < 8 {
            observed.push(hit);
        }
        set_pad(harness, &[], 0.0);
        harness.app.update();
        if let Some(InventoryCellHit::Player(slot)) = hit {
            assert!(
                harness
                    .app
                    .world()
                    .resource::<PlayerRuntime>()
                    .inventory
                    .ledger()
                    .displayed_stack(slot)
                    .is_some_and(|stack| stack.count > 0)
            );
            return InventoryCellHit::Player(slot);
        }
    }
    panic!("D-pad navigation did not reach an occupied player cell: {observed:?}");
}

#[test]
fn held_inventory_button_on_open_does_not_dispatch_or_close() {
    let Some(mut harness) = harness(
        "held_inventory_button_on_open_does_not_dispatch_or_close",
        false,
    ) else {
        return;
    };
    set_pad(&mut harness, &[GamepadButton::North], 0.0);
    harness.app.update();
    assert!(
        harness
            .app
            .world()
            .resource::<UiRuntimeResource>()
            .inventory_open()
    );
    assert_eq!(
        request_count(&harness),
        0,
        "the opening hold is not replayed"
    );
    harness.app.update();
    assert!(
        harness
            .app
            .world()
            .resource::<UiRuntimeResource>()
            .inventory_open()
    );
    assert_eq!(
        request_count(&harness),
        0,
        "a held North does not close or transfer"
    );

    set_pad(&mut harness, &[], 0.0);
    harness.app.update();
    set_pad(&mut harness, &[GamepadButton::North], 0.0);
    harness.app.update();
    assert!(
        harness
            .app
            .world()
            .resource::<UiRuntimeResource>()
            .inventory_open(),
        "North is quick transfer here, not close"
    );
    assert_eq!(request_count(&harness), 0, "no target was selected");
}

#[test]
fn dpad_primary_admits_once_and_stationary_os_cursor_cannot_replace_virtual_pointer() {
    let Some(mut harness) = harness(
        "dpad_primary_admits_once_and_stationary_os_cursor_cannot_replace_virtual_pointer",
        true,
    ) else {
        return;
    };
    update_neutral(&mut harness);

    let hit = navigate_to_player_cell(&mut harness);
    assert!(matches!(hit, InventoryCellHit::Player(_)));
    let runtime = harness.app.world().resource::<UiRuntimeResource>();
    let virtual_pointer = runtime
        .inventory_pointer_gui()
        .expect("D-pad focus publishes a virtual pointer");
    let presentation = &harness.app.world().resource::<UiPresentationResource>().0;
    let os_pointer = presentation
        .inventory_gui_point(UiPoint::new(1.0, 1.0).unwrap(), VIEWPORT, 1.0)
        .unwrap();
    assert_ne!(
        virtual_pointer, os_pointer,
        "the OS cursor stayed stationary"
    );

    set_pad(&mut harness, &[], 0.0);
    harness.app.update();
    let before = request_count(&harness);
    set_pad(&mut harness, &[GamepadButton::South], 0.0);
    harness.app.update();
    assert_eq!(
        request_count(&harness),
        before + 1,
        "Primary admits once on press"
    );

    harness.app.update();
    assert_eq!(
        request_count(&harness),
        before + 1,
        "holding Primary does not repeat"
    );
}

#[test]
fn same_frame_back_close_quarantines_gameplay_movement_and_world_drop() {
    let Some(mut harness) = harness(
        "same_frame_back_close_quarantines_gameplay_movement_and_world_drop",
        true,
    ) else {
        return;
    };
    update_neutral(&mut harness);
    let before = request_count(&harness);
    set_pad(
        &mut harness,
        &[GamepadButton::East, GamepadButton::DPadDown],
        1.0,
    );
    harness.app.update();

    assert!(
        !harness
            .app
            .world()
            .resource::<UiRuntimeResource>()
            .inventory_open(),
        "Back closes the container"
    );
    assert_eq!(
        request_count(&harness),
        before,
        "Drop cannot reach world inventory"
    );
    let snapshot = harness.app.world().resource::<SemanticInputSnapshot>();
    assert_eq!(snapshot.movement(), [0.0; 2]);
    assert!(!snapshot.phase(Action::MoveForward).held);
    assert!(!snapshot.phase(Action::MoveForward).pressed);
}

#[test]
fn simultaneous_mouse_click_and_primary_admit_only_the_mouse_action() {
    let Some(mut harness) = harness(
        "simultaneous_mouse_click_and_primary_admit_only_the_mouse_action",
        true,
    ) else {
        return;
    };
    update_neutral(&mut harness);
    let point = pointer_hit(
        &harness.app.world().resource::<UiPresentationResource>().0,
        InventoryCellHit::Player(0),
    );
    harness
        .app
        .world_mut()
        .get_mut::<Window>(harness.window)
        .unwrap()
        .set_cursor_position(Some(bevy::math::Vec2::new(point[0], point[1])));
    set_pad(&mut harness, &[GamepadButton::South], 0.0);
    harness
        .app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    harness
        .app
        .world_mut()
        .write_message(WindowEvent::MouseButtonInput(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window: harness.window,
        }));
    harness.app.update();

    assert!(
        harness
            .app
            .world()
            .resource::<UiRuntimeResource>()
            .inventory_open()
    );
    assert_eq!(
        request_count(&harness),
        1,
        "mouse takeover suppresses controller Primary"
    );
}
