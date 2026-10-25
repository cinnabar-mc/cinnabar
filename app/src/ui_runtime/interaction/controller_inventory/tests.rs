use super::*;
use bevy::{
    app::{App, Update},
    window::CursorGrabMode,
};
use launcher::menu::settings_options::SettingId;

fn pad(buttons: &[GamepadButton], left_stick: [f32; 2]) -> Gamepad {
    let mut pad = Gamepad::default();
    for button in buttons {
        pad.digital_mut().press(*button);
    }
    pad.analog_mut().set(GamepadAxis::LeftStickX, left_stick[0]);
    pad.analog_mut().set(GamepadAxis::LeftStickY, left_stick[1]);
    pad
}

#[test]
fn a_resting_os_cursor_never_takes_the_pointer() {
    let mut mouse = MouseTracker::default();
    let at = Vec2::new(40.0, 40.0);
    assert_eq!(mouse.observe(Some(at), false), None);
    for _ in 0..10 {
        assert_eq!(mouse.observe(Some(at + Vec2::splat(1.0)), false), None);
    }
}

#[test]
fn real_movement_or_a_button_edge_takes_the_pointer_once() {
    let mut mouse = MouseTracker::default();
    mouse.observe(Some(Vec2::ZERO), false);
    let moved = Vec2::new(10.0, 0.0);
    assert_eq!(mouse.observe(Some(moved), false), Some(moved));
    // The takeover re-anchors, so the same resting position is not new activity.
    assert_eq!(mouse.observe(Some(moved), false), None);
    assert_eq!(mouse.observe(Some(moved), true), Some(moved));
}

#[test]
fn slow_deliberate_movement_accumulates_to_a_takeover() {
    let mut mouse = MouseTracker::default();
    mouse.observe(Some(Vec2::ZERO), false);
    let step = MOUSE_TAKEOVER_DISTANCE / 4.0;
    let taken =
        (1..=8).find_map(|index| mouse.observe(Some(Vec2::new(step * index as f32, 0.0)), false));
    assert!(taken.is_some());
}

#[test]
fn a_withdrawn_cursor_forgets_its_anchor() {
    let mut mouse = MouseTracker::default();
    mouse.observe(Some(Vec2::ZERO), false);
    assert_eq!(mouse.observe(None, true), None);
    // Re-entering far away is a fresh anchor, not a jump from the old position.
    assert_eq!(mouse.observe(Some(Vec2::new(300.0, 0.0)), false), None);
}

#[test]
fn only_safe_actions_have_physical_positions() {
    let bound: Vec<_> = ContainerControllerAction::ALL
        .into_iter()
        .filter(|action| action_button(*action).is_some())
        .collect();
    assert_eq!(
        bound,
        [
            ContainerControllerAction::Primary,
            ContainerControllerAction::HalfOrPlaceOne,
            ContainerControllerAction::QuickTransfer,
        ]
    );
}

/// The default layout and the saved A/B swap, read through the production role mapping.
fn layouts() -> [SettingsOptions; 2] {
    let mut swapped = SettingsOptions::default();
    assert!(swapped.set(SettingId::SwapGamepadAbButtons.index(), 1));
    [SettingsOptions::default(), swapped]
}

/// One physical press of `buttons` under `options`.
fn read(buttons: &[GamepadButton], options: &SettingsOptions) -> PadSample {
    sample_pad(&pad(buttons, [0.0; 2]), |button| {
        role_button(Some(options), button)
    })
}

#[test]
fn physical_buttons_read_roles_through_the_saved_swap() {
    let [default, swapped] = layouts();
    // North is quick transfer by default, and it is not moved by the A/B swap.
    assert_eq!(
        read(&[GamepadButton::North], &default).actions,
        [false, false, true, false, false, false]
    );
    // With A/B swapped, the East button carries Primary and the South button carries Back.
    let east = read(&[GamepadButton::East], &swapped);
    assert!(east.actions[0]);
    assert!(!east.back_edge);
    let south = read(&[GamepadButton::South], &swapped);
    assert!(!south.actions[0]);
    assert!(south.back_edge);
}

#[test]
fn each_face_button_drives_exactly_one_role_under_every_layout() {
    let face = [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::North,
        GamepadButton::West,
    ];
    for options in layouts() {
        for button in face {
            let sample = read(&[button], &options);
            let roles =
                sample.actions.iter().filter(|held| **held).count() + usize::from(sample.back_edge);
            assert_eq!(roles, 1, "{button:?}");
        }
    }
}

#[test]
fn unbound_buttons_and_sticks_read_no_role() {
    let unbound = [
        GamepadButton::LeftThumb,
        GamepadButton::RightThumb,
        GamepadButton::LeftTrigger,
        GamepadButton::RightTrigger,
        GamepadButton::Select,
        GamepadButton::Start,
    ];
    for options in layouts() {
        for button in unbound {
            let sample = read(&[button], &options);
            assert!(sample.active, "{button:?} is still input");
            assert_eq!(sample.actions, [false; 6], "{button:?}");
            assert!(!sample.back_edge, "{button:?}");
        }
        let mut stick = Gamepad::default();
        stick.analog_mut().set(GamepadAxis::RightStickX, 1.0);
        let sample = sample_pad(&stick, |button| role_button(Some(&options), button));
        assert!(sample.active);
        assert_eq!(sample.actions, [false; 6]);
    }
}

#[test]
fn only_a_face_button_inventory_binding_yields_to_a_role() {
    let [default, swapped] = layouts();
    let gamepad = |code: u8| Some(PhysicalControl::GamepadButton(code));
    // The default Inventory binding is North, a face button that quick transfer reads.
    assert!(inventory_control_reads_role(&default, gamepad(2)));
    for face in 0..4 {
        assert!(inventory_control_reads_role(&swapped, gamepad(face)));
    }
    // Shoulders, triggers, thumbs, menu buttons and the D-pad keep the toggle.
    for other in [4, 5, 6, 7, 8, 9, 11, 12, 13, 14] {
        assert!(
            !inventory_control_reads_role(&default, gamepad(other)),
            "{other}"
        );
    }
    assert!(!inventory_control_reads_role(&default, None));
    assert!(!inventory_control_reads_role(
        &default,
        Some(PhysicalControl::KeyboardUsage(0x08))
    ));
}

#[test]
fn back_is_an_edge_and_the_stick_maps_down_positive() {
    let sample = sample_pad(&pad(&[GamepadButton::East], [0.0, 1.0]), |button| button);
    assert!(sample.back_edge);
    assert_eq!(sample.axes, [0.0, -1.0]);
    assert!(sample.active);
    assert!(!sample_pad(&pad(&[], [0.0; 2]), |button| button).active);
}

#[test]
fn the_first_active_controller_keeps_ownership() {
    assert_eq!(select_owner(&[(7, true), (3, true)], None), Some(3));
    assert_eq!(select_owner(&[(7, true), (3, true)], Some(7)), Some(7));
    // An idle owner yields to an active pad, and is kept when nothing else is active.
    assert_eq!(select_owner(&[(7, false), (3, true)], Some(7)), Some(3));
    assert_eq!(select_owner(&[(7, false), (3, false)], Some(7)), Some(7));
}

#[test]
fn a_disconnected_owner_is_replaced_and_no_pads_means_no_owner() {
    assert_eq!(select_owner(&[(3, false)], Some(7)), Some(3));
    assert_eq!(select_owner(&[], Some(7)), None);
    assert_eq!(select_owner(&[(9, false), (4, false)], None), Some(4));
}

#[test]
fn geometry_revisions_advance_only_when_the_layout_changes() {
    let mut revisions = RevisionTracker::<Vec<u8>>::default();
    let first = revisions.observe(&vec![1, 2]);
    assert_eq!(revisions.observe(&vec![1, 2]), first);
    let second = revisions.observe(&vec![1, 3]);
    assert!(second > first);
    assert_eq!(revisions.observe(&vec![1, 3]), second);
}

#[test]
fn a_real_mouse_takeover_beats_a_retained_virtual_pointer() {
    let mut input = ControllerInventoryInput::default();
    assert_eq!(input.pointer(), EffectivePointer::Os);
    input.virtual_gui = Some([10.0, 20.0]);
    assert_eq!(input.pointer(), EffectivePointer::Virtual([10.0, 20.0]));
    input.takeover = Some(Vec2::new(5.0, 5.0));
    assert_eq!(input.pointer(), EffectivePointer::Os);
    assert_eq!(effective_pointer(None), EffectivePointer::Os);
}

#[test]
fn deactivation_cancels_the_virtual_pointer_gestures_and_edges() {
    let mut runtime = UiRuntime::new(1);
    let mut input = ControllerInventoryInput::default();
    input.virtual_gui = Some([1.0, 2.0]);
    input.back.armed = true;
    runtime.screen_state_mut().container_close_armed = true;
    runtime.set_inventory_pointer_gui(Some([1.0, 2.0]));

    input.deactivate(&mut runtime);

    assert_eq!(input.pointer(), EffectivePointer::Os);
    assert!(!input.back.armed);
    assert!(!runtime.screen_state().container_close_armed);
    assert_eq!(runtime.inventory_pointer_gui(), None);
    assert_eq!(runtime.inventory_controller_focus(), None);
}

#[test]
fn back_needs_neutrality_within_the_same_generation_and_controller() {
    let mut gate = BackGate::default();
    // Held at open: no edge acts until a neutral sample arms the gate.
    assert!(!gate.accept((1, 7), false, true, false));
    assert!(!gate.accept((1, 7), true, false, false));
    assert!(gate.accept((1, 7), false, true, false));
}

#[test]
fn a_new_controller_or_screen_lifetime_disarms_back() {
    let mut gate = BackGate::default();
    gate.accept((1, 7), true, false, false);
    // Another pad, or a close/reopen, must be seen neutral itself first.
    assert!(!gate.accept((1, 8), false, true, false));
    gate.accept((1, 8), true, false, false);
    assert!(!gate.accept((2, 8), false, true, false));
    gate.accept((2, 8), true, false, false);
    assert!(gate.accept((2, 8), false, true, false));
}

#[test]
fn a_mouse_takeover_disarms_back_and_swallows_its_edge() {
    let mut gate = BackGate::default();
    gate.accept((1, 7), true, false, false);
    assert!(!gate.accept((1, 7), true, true, true));
    assert!(!gate.accept((1, 7), false, true, false));
}

/// Builds an isolated visibility owner without the desktop camera policy.
fn cursor_app(
    focused: bool,
    open: bool,
    controller_cursor_visible: bool,
    input: ControllerInventoryInput,
) -> (App, bevy::prelude::Entity) {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    if open {
        runtime.toggle_inventory(&mut player, None);
    }
    runtime.set_inventory_controller_cursor_visible(controller_cursor_visible);
    let mut app = App::new();
    app.insert_resource(UiRuntimeResource(runtime))
        .insert_resource(input)
        .add_systems(Update, hide_os_cursor_for_controller);
    let entity = app
        .world_mut()
        .spawn((
            Window {
                focused,
                ..Default::default()
            },
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: true,
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    (app, entity)
}

/// Runs one visibility sample and reports cursor visibility and grab mode.
fn cursor_after_hide(
    focused: bool,
    open: bool,
    controller_cursor_visible: bool,
    input: ControllerInventoryInput,
) -> (bool, CursorGrabMode) {
    let (mut app, entity) = cursor_app(focused, open, controller_cursor_visible, input);
    app.update();
    let cursor = app
        .world()
        .get::<CursorOptions>(entity)
        .expect("primary window cursor");
    (cursor.visible, cursor.grab_mode)
}

/// A controller pointer that leads: a virtual cursor with no mouse takeover.
fn controller_leads() -> ControllerInventoryInput {
    let mut input = ControllerInventoryInput::default();
    input.virtual_gui = Some([10.0, 20.0]);
    input
}

#[test]
fn a_leading_controller_pointer_hides_the_os_cursor_without_touching_capture() {
    let (visible, grab) = cursor_after_hide(true, true, true, controller_leads());
    assert!(!visible);
    assert_eq!(grab, CursorGrabMode::Locked);
}

#[test]
fn mouse_handoff_and_virtual_pointer_withdrawal_restore_visibility_without_a_camera() {
    for mouse_takeover in [true, false] {
        let (mut app, window) = cursor_app(true, true, true, controller_leads());
        app.world_mut()
            .get_mut::<CursorOptions>(window)
            .unwrap()
            .grab_mode = CursorGrabMode::None;
        app.update();
        assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
        let mut input = app.world_mut().resource_mut::<ControllerInventoryInput>();
        if mouse_takeover {
            input.takeover = Some(Vec2::new(5.0, 5.0));
        } else {
            input.virtual_gui = None;
        }
        drop(input);
        app.update();
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert!(cursor.visible);
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    }
}

#[test]
fn pointer_handoff_never_unhides_captured_gameplay_but_restores_after_unlock() {
    let (mut app, window) = cursor_app(true, true, true, controller_leads());
    app.update();
    app.world_mut()
        .resource_mut::<ControllerInventoryInput>()
        .virtual_gui = None;
    app.update();
    assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
    app.world_mut()
        .get_mut::<CursorOptions>(window)
        .unwrap()
        .grab_mode = CursorGrabMode::None;
    app.update();
    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
}

#[test]
fn the_os_cursor_is_left_to_the_camera_unless_every_controller_condition_holds() {
    assert!(
        cursor_after_hide(false, true, true, controller_leads()).0,
        "unfocused window"
    );
    assert!(
        cursor_after_hide(true, false, true, controller_leads()).0,
        "closed container"
    );
    assert!(
        cursor_after_hide(true, true, false, controller_leads()).0,
        "no controller cursor"
    );
    let mut mouse = controller_leads();
    mouse.takeover = Some(Vec2::new(5.0, 5.0));
    assert!(
        cursor_after_hide(true, true, true, mouse).0,
        "mouse takeover"
    );
    assert!(
        cursor_after_hide(true, true, true, ControllerInventoryInput::default()).0,
        "no virtual pointer"
    );
}

#[test]
fn ui_ownership_latches_for_a_same_frame_close_or_reopen() {
    let mut input = ControllerInventoryInput::default();
    input.start_open = true;
    input.start_generation = 4;
    input.pad_in_use = true;
    // Closed by earlier keyboard routing before this system ran.
    input.latch_ui_owned(false, 5);
    assert!(input.consumed_gameplay_edges());
    // A later early-return observation cannot clear the latch.
    input.latch_ui_owned(false, 5);
    assert!(input.consumed_gameplay_edges());
}

#[test]
fn a_close_and_reopen_within_the_frame_latches_from_the_generation_alone() {
    let mut input = ControllerInventoryInput::default();
    input.start_generation = 4;
    input.pad_in_use = true;
    input.latch_ui_owned(false, 6);
    assert!(input.consumed_gameplay_edges());
}

#[test]
fn an_idle_frame_or_an_unrelated_closed_screen_does_not_latch() {
    let mut input = ControllerInventoryInput::default();
    input.start_generation = 4;
    input.pad_in_use = true;
    input.latch_ui_owned(false, 4);
    assert!(!input.consumed_gameplay_edges());
    input.start_open = true;
    input.pad_in_use = false;
    input.latch_ui_owned(true, 4);
    assert!(!input.consumed_gameplay_edges());
}
