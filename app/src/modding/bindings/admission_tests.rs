//! Resolves exported chords as a guest would against real finalized native input.
use super::*;
use crate::semantic_controls::{
    PendingDeviceFrame, SemanticInputRuntime, SemanticRouteState,
    finalize_semantic_input_after_ui_authority, route_semantic_input,
};
use bevy::prelude::*;
use semantic_input::finalized::FinalizedInput;
use semantic_input::{
    Action, ControllerFrame, DesktopEvent, DesktopEvents, DeviceFrame, InputChord,
    KeyboardMouseFrame, ModifierChord,
};

/// Constructs a desktop sample with complete held modifier keys, plus optional arrival edges.
fn desktop(keys: &[u16], buttons: &[u8]) -> DeviceFrame {
    DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            keys: keys.to_vec(),
            mouse_buttons: buttons.to_vec(),
            ..Default::default()
        }),
        desktop_events: Some(DesktopEvents::default()),
        ..Default::default()
    }
}

/// Builds player settings with the supplied gameplay chords.
fn settings(chords: &[(Action, PhysicalControl, ModifierChord)]) -> ControlSettings {
    ControlSettings::new(
        chords
            .iter()
            .map(|&(action, control, modifiers)| ActionBinding {
                action,
                context: InputContext::Gameplay,
                chord: InputChord { control, modifiers },
            })
            .collect(),
        0.5,
        1.,
        1.,
        false,
        false,
        0.15,
        0.15,
    )
    .unwrap()
}

/// Matches all modifier groups and suppresses less-specific chords on the same control.
fn guest_held(bindings: &InputBindings, keys: &[String], action: &str) -> bool {
    let has = |name: &str| keys.iter().any(|key| key == name);
    let held = BindingModifiers {
        shift: has("ShiftLeft") || has("ShiftRight"),
        control: has("ControlLeft") || has("ControlRight"),
        alt: has("AltLeft") || has("AltRight"),
        super_key: has("SuperLeft") || has("SuperRight"),
    };
    let satisfied = |m: &BindingModifiers| {
        (!m.shift || held.shift)
            && (!m.control || held.control)
            && (!m.alt || held.alt)
            && (!m.super_key || held.super_key)
    };
    let specificity = |m: &BindingModifiers| {
        u8::from(m.shift) + u8::from(m.control) + u8::from(m.alt) + u8::from(m.super_key)
    };
    bindings.entries.iter().any(|binding| {
        binding.action == action
            && binding.context == BindingContext::Gameplay
            && has(&binding.key)
            && satisfied(&binding.modifiers)
            && !bindings.entries.iter().any(|other| {
                other.context == binding.context
                    && other.key == binding.key
                    && satisfied(&other.modifiers)
                    && specificity(&other.modifiers) > specificity(&binding.modifiers)
            })
    })
}

/// Checks the physical gates consumed by pulse-attack and pulse-use against guest resolution.
pub(crate) fn assert_equivalent(
    snapshot: semantic_input::ActionSnapshot,
    controls: &ControlSettings,
    attack: bool,
    use_held: bool,
) -> Vec<String> {
    let input = FinalizedInput::from_finalized(snapshot);
    let keys = admitted_keys(input.snapshot(), true);
    let bindings = Bindings::default().sample(controls);
    for (action, expected) in [(Action::Attack, attack), (Action::Use, use_held)] {
        assert_eq!(input.phase(action).held, expected, "native {action:?}");
        assert_eq!(
            guest_held(&bindings, &keys, action.binding_name()),
            expected,
            "guest {action:?}: {keys:?}"
        );
    }
    assert!(admitted_keys(input.snapshot(), false).is_empty());
    assert_pulse_gates(&input, &keys, attack, use_held);
    keys
}

/// Runs both real host pulse imports against the same finalized physical hold facts.
fn assert_pulse_gates(input: &FinalizedInput, keys: &[String], attack: bool, use_held: bool) {
    let directory = tempfile::tempdir().unwrap();
    let source = include_str!("pulse_gates.wat")
        .replace("$ATTACK_DENIED", if attack { "0" } else { "1" })
        .replace("$USE_DENIED", if use_held { "0" } else { "1" });
    let mut host = mod_host::ModHost::load_snapshot_with_grants(
        &directory.path().join("pulse-gates.wat"),
        source.as_bytes(),
        mod_host::ModGrants {
            controls: true,
            interaction: true,
            item_use: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut controls = mod_host::empty_controls();
    controls.focused = true;
    controls.gameplay = true;
    controls.keys_admitted = keys.to_vec();
    host.observe_gameplay(mod_host::GameplayFacts {
        use_held: input.phase(Action::Use).held,
        ..Default::default()
    })
    .unwrap();
    host.frame_with_controls(
        false,
        Some(mod_host::GameplaySnapshot {
            session: 1,
            dimension: 0,
            eye: mod_host::GameplayVector3 {
                x: 0.,
                y: 64.,
                z: 0.,
            },
            yaw: 0.,
            pitch: 0.,
            attack_held: input.phase(Action::Attack).held,
            frame_seconds: 0.016,
            players: Vec::new(),
        }),
        controls,
    )
    .unwrap();
    assert_eq!(host.attack_pulse_requested(), attack);
    assert_eq!(host.use_pulse_requested(), use_held);
}

#[test]
fn normal_holds_rebind_quarantine_and_release_repress_share_the_gate() {
    let mut runtime = SemanticInputRuntime::default();
    let initial = ControlSettings::default();
    let frame = desktop(&[0x0e], &[1, 2]);
    let keys = assert_equivalent(
        runtime.route_and_finalize(frame.clone()).unwrap(),
        &initial,
        true,
        true,
    );
    assert_eq!(keys, ["KeyK", "MouseLeft", "MouseRight"]);
    let rebound = settings(&[(
        Action::Attack,
        PhysicalControl::KeyboardUsage(0x0e),
        ModifierChord::default(),
    )]);
    runtime.replace_bindings(rebound.clone()).unwrap();
    for _ in 0..3 {
        assert!(
            assert_equivalent(
                runtime.route_and_finalize(frame.clone()).unwrap(),
                &rebound,
                false,
                false
            )
            .is_empty()
        );
    }
    assert_equivalent(
        runtime.route_and_finalize(desktop(&[], &[])).unwrap(),
        &rebound,
        false,
        false,
    );
    assert_eq!(
        assert_equivalent(
            runtime.route_and_finalize(frame).unwrap(),
            &rebound,
            true,
            false
        ),
        ["KeyK", "MouseLeft", "MouseRight"]
    );
}

#[test]
fn focus_loss_and_context_ownership_quarantine_held_controls() {
    for context_change in [false, true] {
        let mut runtime = SemanticInputRuntime::default();
        let settings = ControlSettings::default();
        assert_equivalent(
            runtime.route_and_finalize(desktop(&[], &[1, 2])).unwrap(),
            &settings,
            true,
            true,
        );
        if context_change {
            runtime.set_context(InputContext::UiFocused);
        }
        let mut frame = desktop(&[], &[1, 2]);
        frame.window_focus_lost = !context_change;
        assert!(
            assert_equivalent(
                runtime.route_and_finalize(frame).unwrap(),
                &settings,
                false,
                false
            )
            .is_empty()
        );
        runtime.set_context(InputContext::Gameplay);
        assert!(
            assert_equivalent(
                runtime.route_and_finalize(desktop(&[], &[1, 2])).unwrap(),
                &settings,
                false,
                false
            )
            .is_empty()
        );
    }
}

#[test]
fn screen_consumption_barrier_is_published_before_admission_is_copied() {
    let mut app = App::new();
    app.init_resource::<SemanticInputRuntime>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticRouteState>()
        .init_resource::<client_state::SemanticInputSnapshot>()
        .init_resource::<client_state::ScreenInputConsumed>()
        .add_systems(
            Update,
            (
                route_semantic_input,
                finalize_semantic_input_after_ui_authority,
            )
                .chain(),
        );
    for consumed in [false, true, false, false] {
        app.world_mut()
            .resource_mut::<PendingDeviceFrame>()
            .replace(desktop(&[], &[1, 2]));
        app.world_mut()
            .resource_mut::<client_state::ScreenInputConsumed>()
            .0 = consumed;
        app.update();
        let snapshot = app
            .world()
            .resource::<client_state::SemanticInputSnapshot>()
            .snapshot()
            .unwrap()
            .clone();
        let expected = !consumed && snapshot.frame_sequence.get() == 1;
        assert_equivalent(snapshot, &ControlSettings::default(), expected, expected);
    }
}

#[test]
fn active_controller_selects_gamepad_while_desktop_key_stays_down() {
    let controls = settings(&[(
        Action::Attack,
        PhysicalControl::KeyboardUsage(0x0e),
        ModifierChord::default(),
    )]);
    let mut runtime = SemanticInputRuntime::default();
    runtime.replace_bindings(controls.clone()).unwrap();
    let mut frame = desktop(&[0x0e], &[]);
    assert_equivalent(
        runtime.route_and_finalize(frame.clone()).unwrap(),
        &controls,
        true,
        false,
    );
    frame.controllers = vec![ControllerFrame {
        device_id: 7,
        ..Default::default()
    }];
    // Reconnect rearming requires a neutral sample before controller activity is eligible.
    assert_equivalent(
        runtime.route_and_finalize(frame.clone()).unwrap(),
        &controls,
        true,
        false,
    );
    frame.controllers[0].axes[0] = 1.;
    let snapshot = runtime.route_and_finalize(frame).unwrap();
    assert_eq!(snapshot.input_mode, semantic_input::InputMode::GamePad);
    assert!(assert_equivalent(snapshot, &controls, false, false).is_empty());
}

#[test]
fn modifier_precedence_uses_admitted_held_sides_and_preserves_press_edges() {
    let shift = ModifierChord {
        shift: true,
        ..Default::default()
    };
    use semantic_input::KeyboardKey;
    for (required, sides) in [
        (shift, [KeyboardKey::ShiftLeft, KeyboardKey::ShiftRight]),
        (
            ModifierChord {
                control: true,
                ..Default::default()
            },
            [KeyboardKey::ControlLeft, KeyboardKey::ControlRight],
        ),
        (
            ModifierChord {
                alt: true,
                ..Default::default()
            },
            [KeyboardKey::AltLeft, KeyboardKey::AltRight],
        ),
        (
            ModifierChord {
                super_key: true,
                ..Default::default()
            },
            [KeyboardKey::SuperLeft, KeyboardKey::SuperRight],
        ),
    ] {
        let controls = settings(&[
            (
                Action::Attack,
                PhysicalControl::MouseButton(1),
                ModifierChord::default(),
            ),
            (Action::Use, PhysicalControl::MouseButton(1), required),
        ]);
        for key in sides {
            let side = key.usage().unwrap();
            let mut runtime = SemanticInputRuntime::default();
            runtime.replace_bindings(controls.clone()).unwrap();
            let frame = desktop(&[side], &[1]);
            assert_equivalent(
                runtime.route_and_finalize(frame.clone()).unwrap(),
                &controls,
                false,
                true,
            );
            runtime.consume_control(PhysicalControl::KeyboardUsage(side));
            assert_eq!(
                assert_equivalent(
                    runtime.route_and_finalize(frame).unwrap(),
                    &controls,
                    true,
                    false
                ),
                ["MouseLeft"]
            );
        }
    }
    let controls = settings(&[
        (
            Action::Attack,
            PhysicalControl::MouseButton(1),
            ModifierChord::default(),
        ),
        (Action::Use, PhysicalControl::MouseButton(1), shift),
    ]);
    // A modifier released in this batch affects its press edge, never the final hold.
    let mut runtime = SemanticInputRuntime::default();
    runtime.replace_bindings(controls.clone()).unwrap();
    let mut frame = desktop(&[], &[1]);
    frame.keyboard_mouse.as_mut().unwrap().modifiers = shift;
    let events = frame.desktop_events.as_mut().unwrap();
    events.set_initial_modifiers(shift);
    events.push(DesktopEvent::Button {
        control: PhysicalControl::MouseButton(1),
        pressed: true,
        modifiers: shift,
    });
    events.push(DesktopEvent::Button {
        control: PhysicalControl::KeyboardUsage(0xe1),
        pressed: false,
        modifiers: ModifierChord::default(),
    });
    let snapshot = runtime.route_and_finalize(frame).unwrap();
    assert!(snapshot.phases[Action::Use as usize].pressed);
    assert_equivalent(snapshot, &controls, true, false);
}

#[test]
fn ordered_consumed_release_repress_rearms_admission_in_the_same_batch() {
    let mut runtime = SemanticInputRuntime::default();
    runtime.consume_control(PhysicalControl::MouseButton(1));
    let mut frame = desktop(&[], &[1]);
    for pressed in [true, false, true] {
        frame
            .desktop_events
            .as_mut()
            .unwrap()
            .push(DesktopEvent::Button {
                control: PhysicalControl::MouseButton(1),
                pressed,
                modifiers: ModifierChord::default(),
            });
    }
    assert_eq!(
        assert_equivalent(
            runtime.route_and_finalize(frame).unwrap(),
            &ControlSettings::default(),
            true,
            false
        ),
        ["MouseLeft"]
    );
}
