//! Gameplay touch controls: each finger keeps the control it pressed until it lifts.

use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::touch::{TouchInput, TouchPhase, Touches},
    prelude::{Local, Query, Res, ResMut, With},
    window::{PrimaryWindow, Window},
};
use semantic_input::{TouchContact, TouchControlRegion, TouchControlState, touch};

use crate::semantic_controls::SemanticTouchTargets;
use client_shell::{
    menu::MenuRuntime,
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};

/// Look strength per viewport fraction dragged, wider across for a landscape screen.
/// Provisional until measured against the native touch look curve.
const LOOK_GAIN: [f32; 2] = [4.0, 2.0];

/// One frame of touch events applied to the captured fingers.
#[derive(Debug, Default, PartialEq)]
struct TouchFrame {
    /// Lifted contacts' last samples, one per id.
    lifted: Vec<TouchContact>,
    /// Controls a finger newly captured, even if it lifted again in the same frame.
    pressed: Vec<u16>,
    /// Mod buttons whose finger lifted inside them, in completion order.
    mod_taps: Vec<u16>,
}

/// Dropdown choices retain their contact for pressed feedback until a completed tap.
fn utility_choice(hit_id: u16) -> bool {
    matches!(
        hit_id,
        touch::EMOTE | touch::DEBUG_OVERLAY | touch::PLAYER_LIST
    )
}

/// Mod buttons, like dropdown choices, hold their finger for pressed art and complete on release;
/// they never reach the gameplay router.
fn completes_on_release(hit_id: u16) -> bool {
    utility_choice(hit_id) || touch::mod_button_slot(hit_id).is_some()
}

/// Cancels fingers held on mod buttons, so a lift after the buttons change cannot complete a
/// tap on a different button.
fn cancel_mod_button_contacts(state: &mut TouchControlState) {
    for contact in state.sample() {
        if contact
            .hit_id
            .is_some_and(|id| touch::mod_button_slot(id).is_some())
        {
            state.release(contact.contact_id);
        }
    }
}

/// Applies touch events in arrival order, so an id Android reuses after a lift starts a fresh
/// contact. A press on a screen-opening control stops and returns that control.
fn apply_touch_events(
    state: &mut TouchControlState,
    events: impl IntoIterator<Item = (u64, TouchPhase, [f32; 2])>,
    region: impl Fn([f32; 2]) -> Option<TouchControlRegion>,
) -> Result<TouchFrame, u16> {
    let mut frame = TouchFrame::default();
    for (id, phase, position) in events {
        match phase {
            TouchPhase::Started => {
                let region = region(position);
                if let Some(hit_id) = region.map(|region| region.hit_id).filter(|hit_id| {
                    matches!(
                        *hit_id,
                        touch::INVENTORY
                            | touch::MENU
                            | touch::CHAT
                            | touch::UTILITIES
                            | touch::DISMISS_UTILITIES
                    )
                }) {
                    return Err(hit_id);
                }
                if state.begin(id, position, 0, region)
                    && let Some(region) = region
                    && !completes_on_release(region.hit_id)
                {
                    frame.pressed.push(region.hit_id);
                }
            }
            TouchPhase::Moved => {
                state.move_to(id, position, 0);
            }
            TouchPhase::Ended => {
                state.move_to(id, position, 0);
                if let Some(sample) = state.end(id) {
                    if let Some(hit_id) = sample.hit_id.filter(|id| utility_choice(*id)) {
                        if region(position).map(|hit| hit.hit_id) == Some(hit_id) {
                            return Err(hit_id);
                        }
                        continue;
                    }
                    if let Some(hit_id) = sample
                        .hit_id
                        .filter(|id| touch::mod_button_slot(*id).is_some())
                    {
                        if region(position).map(|hit| hit.hit_id) == Some(hit_id) {
                            frame.mod_taps.push(hit_id);
                        }
                        continue;
                    }
                    // A reused id may lift twice in one frame; a frame carries each id once.
                    frame
                        .lifted
                        .retain(|earlier: &TouchContact| earlier.contact_id != id);
                    frame.lifted.push(sample);
                }
            }
            TouchPhase::Canceled => state.release(id),
        }
    }
    Ok(frame)
}

#[allow(
    clippy::too_many_arguments,
    reason = "UI, menu and player authority are borrowed separately."
)]
pub(crate) fn drive_gameplay_touch_targets(
    mut touches: ResMut<Touches>,
    touch_input: Option<Res<Messages<TouchInput>>>,
    mut touch_cursor: Local<MessageCursor<TouchInput>>,
    mut ui: ResMut<UiRuntimeResource>,
    mut targets: ResMut<SemanticTouchTargets>,
    player: Option<ResMut<client_state::PlayerRuntime>>,
    mut menu: Option<ResMut<MenuRuntime>>,
    presentation: Option<ResMut<UiPresentationResource>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut state: Local<TouchControlState>,
    mut mod_button_epoch: Local<u64>,
    world: Option<Res<crate::runtime::world::ClientWorld>>,
) {
    targets.release_all();
    // Every event is consumed each frame, so none replays once gameplay regains input.
    let events: Vec<_> = touch_input
        .as_deref()
        .map(|messages| touch_cursor.read(messages).cloned().collect())
        .unwrap_or_default();
    let Some(mut presentation) = presentation.filter(|view| view.touch_controls_enabled()) else {
        state.clear();
        return;
    };
    let gameplay = match (player.as_deref(), windows.single()) {
        (Some(player), Ok(window)) => (window.focused
            && !ui.ui_focused(player)
            && !menu.as_deref().is_some_and(MenuRuntime::is_visible))
        .then_some(window),
        _ => None,
    };
    let (Some(window), Some(mut player)) = (gameplay, player) else {
        state.clear();
        presentation.set_touch_contacts(Vec::new());
        presentation.close_touch_utilities();
        return;
    };
    // Panels and modals over the world own touches, so mod buttons beneath them never fire.
    let mod_buttons_covered = client_shell::screen_policy::absorbs_input(
        &player,
        Some(&ui),
        menu.as_deref(),
        Some(&presentation),
    );
    if *mod_button_epoch != presentation.mod_touch_epoch() {
        *mod_button_epoch = presentation.mod_touch_epoch();
        cancel_mod_button_contacts(&mut state);
    }
    let size = [window.width().max(1.0), window.height().max(1.0)];
    let events = events.iter().map(|event| {
        let position = [
            (event.position.x / size[0]).clamp(0.0, 1.0),
            (event.position.y / size[1]).clamp(0.0, 1.0),
        ];
        (event.id, event.phase, position)
    });
    let TouchFrame {
        lifted,
        pressed,
        mod_taps,
    } = match apply_touch_events(&mut state, events, |position| {
        presentation.gameplay_touch_region(position, size)
    }) {
        Ok(frame) => frame,
        Err(hit_id) => {
            match hit_id {
                touch::INVENTORY => {
                    let authority = world
                        .as_deref()
                        .and_then(|world| world.stream.as_ref())
                        .map(|stream| stream.authority());
                    let _ = ui.toggle_inventory(&mut player, authority);
                }
                touch::CHAT => {
                    ui.open_chat(&mut player);
                }
                touch::UTILITIES => presentation.toggle_touch_utilities(),
                touch::DEBUG_OVERLAY => {
                    presentation.request_touch_debug_toggle();
                    presentation.close_touch_utilities();
                }
                touch::PLAYER_LIST => {
                    ui.toggle_touch_player_list();
                    presentation.close_touch_utilities();
                }
                touch::DISMISS_UTILITIES => presentation.close_touch_utilities(),
                touch::EMOTE => {
                    ui.emotes_mut().open();
                    presentation.close_touch_utilities();
                }
                _ => {
                    if let Some(menu) = menu.as_mut() {
                        menu.open_pause();
                    }
                }
            }
            // The opened screen must not read this frame's presses as its own taps.
            let started: Vec<_> = touches
                .iter_just_pressed()
                .map(|touch| touch.id())
                .collect();
            for id in started {
                touches.clear_just_pressed(id);
            }
            state.clear();
            presentation.set_touch_contacts(Vec::new());
            return;
        }
    };
    for hit_id in pressed {
        targets.press(hit_id);
    }
    if mod_buttons_covered {
        cancel_mod_button_contacts(&mut state);
    } else {
        for hit_id in mod_taps {
            presentation.complete_mod_touch_tap(hit_id);
        }
    }
    let held = state.sample();
    presentation.set_touch_contacts(held.clone());
    let look_scaled = |sample: TouchContact| {
        if sample.hit_id == Some(touch::LOOK_SURFACE) {
            TouchContact {
                delta: [
                    sample.delta[0] * LOOK_GAIN[0],
                    sample.delta[1] * LOOK_GAIN[1],
                ],
                ..sample
            }
        } else {
            sample
        }
    };
    for sample in held
        .iter()
        .filter(|sample| !sample.hit_id.is_some_and(completes_on_release))
        .cloned()
        .map(look_scaled)
    {
        targets.set_sample(&sample);
    }
    // A lifted contact whose id already carries a new press routes as that press instead.
    for sample in lifted.into_iter().map(look_scaled) {
        if !held
            .iter()
            .any(|other| other.contact_id == sample.contact_id)
        {
            targets.pulse(sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semantic_input::TouchBounds;

    fn stick(_: [f32; 2]) -> Option<TouchControlRegion> {
        Some(TouchControlRegion {
            hit_id: touch::JOYSTICK,
            bounds: TouchBounds {
                min: [0.0; 2],
                max: [1.0; 2],
            },
        })
    }

    /// Android may end a finger and reuse its id for a new press before the next update.
    #[test]
    fn a_reused_touch_id_keeps_its_new_contact() {
        let mut state = TouchControlState::default();
        let at = [0.5, 0.5];
        apply_touch_events(&mut state, [(3, TouchPhase::Started, at)], stick).unwrap();
        let lifted = apply_touch_events(
            &mut state,
            [(3, TouchPhase::Ended, at), (3, TouchPhase::Started, at)],
            stick,
        )
        .unwrap()
        .lifted;
        assert_eq!(lifted.len(), 1);
        let held = state.sample();
        assert_eq!(held.len(), 1);
        assert_eq!(
            (held[0].contact_id, held[0].hit_id),
            (3, Some(touch::JOYSTICK))
        );
    }

    /// A reused id that lifts twice in one frame yields one sample, so the routed frame stays
    /// valid and other fingers keep moving the player.
    #[test]
    fn a_reused_id_lifting_twice_still_routes_other_fingers() {
        use semantic_input::{DeviceFrame, SemanticInputRouter};

        let controls = |position: [f32; 2]| {
            let (hit_id, min, max) = if position[0] < 0.5 {
                (touch::JOYSTICK, [0.0, 0.0], [0.5, 1.0])
            } else {
                (touch::JUMP, [0.5, 0.0], [1.0, 1.0])
            };
            Some(TouchControlRegion {
                hit_id,
                bounds: TouchBounds { min, max },
            })
        };
        let jump = [0.75, 0.5];
        let mut state = TouchControlState::default();
        apply_touch_events(
            &mut state,
            [
                (9, TouchPhase::Started, [0.25, 0.5]),
                (3, TouchPhase::Started, jump),
            ],
            controls,
        )
        .unwrap();
        let lifted = apply_touch_events(
            &mut state,
            [
                (3, TouchPhase::Ended, jump),
                (3, TouchPhase::Started, jump),
                (3, TouchPhase::Ended, jump),
                (9, TouchPhase::Moved, [0.25, 0.0]),
            ],
            controls,
        )
        .unwrap()
        .lifted;
        assert_eq!(lifted.len(), 1);
        let mut touches = state.sample();
        touches.extend(lifted);
        // Production stamps activity before routing; the router rejects sequence zero.
        for (sequence, touch) in touches.iter_mut().enumerate() {
            touch.activity_sequence = sequence as u64 + 1;
        }
        let mut router = SemanticInputRouter::default();
        router
            .route(DeviceFrame {
                touches,
                ..DeviceFrame::default()
            })
            .unwrap();
        assert!(router.finalize().unwrap().movement[1] > 0.0);
    }

    /// A Use tap that starts and lifts within one frame still reports its press, which opens a
    /// held book.
    #[test]
    fn a_tap_within_one_frame_reports_its_press() {
        let mut state = TouchControlState::default();
        let use_button = |_| {
            Some(TouchControlRegion {
                hit_id: touch::USE,
                bounds: TouchBounds {
                    min: [0.0; 2],
                    max: [1.0; 2],
                },
            })
        };
        let frame = apply_touch_events(
            &mut state,
            [
                (4, TouchPhase::Started, [0.5; 2]),
                (4, TouchPhase::Ended, [0.5; 2]),
            ],
            use_button,
        )
        .unwrap();
        assert_eq!(frame.pressed, [touch::USE]);
        assert_eq!(frame.lifted.len(), 1);
        let mut targets = SemanticTouchTargets::default();
        targets.press(touch::USE);
        assert!(targets.pressed(touch::USE) && !targets.pressed(touch::ATTACK));
        targets.release_all();
        assert!(!targets.pressed(touch::USE));
    }

    /// Screen-opening touches stop processing before a later contact can drive gameplay.
    #[test]
    fn screen_controls_open_instead_of_capturing() {
        for hit_id in [
            touch::MENU,
            touch::INVENTORY,
            touch::CHAT,
            touch::UTILITIES,
            touch::DISMISS_UTILITIES,
        ] {
            let mut state = TouchControlState::default();
            let screen = |_| {
                Some(TouchControlRegion {
                    hit_id,
                    bounds: TouchBounds {
                        min: [0.0; 2],
                        max: [1.0; 2],
                    },
                })
            };
            assert_eq!(
                apply_touch_events(
                    &mut state,
                    [
                        (1, TouchPhase::Started, [0.5; 2]),
                        (2, TouchPhase::Started, [0.8; 2]),
                    ],
                    screen
                ),
                Err(hit_id)
            );
            assert!(state.sample().is_empty());
        }
    }

    #[test]
    fn utility_choices_hold_until_release_and_cancel_outside_or_on_focus_loss() {
        for hit_id in [touch::EMOTE, touch::DEBUG_OVERLAY, touch::PLAYER_LIST] {
            let region = |position: [f32; 2]| {
                (position[0] < 0.5).then_some(TouchControlRegion {
                    hit_id,
                    bounds: TouchBounds {
                        min: [0.; 2],
                        max: [0.5, 1.],
                    },
                })
            };
            let mut state = TouchControlState::default();
            let at = [0.25, 0.5];
            let frame =
                apply_touch_events(&mut state, [(1, TouchPhase::Started, at)], region).unwrap();
            assert!(frame.pressed.is_empty());
            assert_eq!(state.sample()[0].hit_id, Some(hit_id));
            assert_eq!(
                apply_touch_events(&mut state, [(1, TouchPhase::Ended, at)], region),
                Err(hit_id)
            );
            assert!(
                apply_touch_events(&mut state, [(1, TouchPhase::Ended, at)], region)
                    .unwrap()
                    .lifted
                    .is_empty()
            );
            for phase in [TouchPhase::Ended, TouchPhase::Canceled] {
                apply_touch_events(&mut state, [(1, TouchPhase::Started, at)], region).unwrap();
                assert!(
                    apply_touch_events(&mut state, [(1, phase, [0.75, 0.5])], region)
                        .unwrap()
                        .lifted
                        .is_empty()
                );
                assert!(state.sample().is_empty());
            }
            apply_touch_events(&mut state, [(1, TouchPhase::Started, at)], region).unwrap();
            state.clear();
            assert!(
                apply_touch_events(&mut state, [(1, TouchPhase::Ended, at)], region)
                    .unwrap()
                    .lifted
                    .is_empty()
            );
        }
    }

    /// A mod button completes on release inside it without interrupting other fingers, and
    /// never becomes a gameplay press.
    #[test]
    fn mod_buttons_tap_on_release_inside_and_keep_other_fingers() {
        let button = touch::mod_button(0).unwrap();
        let controls = |position: [f32; 2]| {
            let (hit_id, min, max) = if position[0] < 0.5 {
                (touch::JOYSTICK, [0.0, 0.0], [0.5, 1.0])
            } else if position[1] < 0.5 {
                (button, [0.5, 0.0], [1.0, 0.5])
            } else {
                return None;
            };
            Some(TouchControlRegion {
                hit_id,
                bounds: TouchBounds { min, max },
            })
        };
        let stick = [0.25, 0.5];
        let inside = [0.75, 0.25];
        let mut state = TouchControlState::default();
        let frame = apply_touch_events(
            &mut state,
            [
                (1, TouchPhase::Started, stick),
                (2, TouchPhase::Started, inside),
            ],
            controls,
        )
        .unwrap();
        assert!(frame.pressed.is_empty() && frame.mod_taps.is_empty());
        assert!(state.sample().iter().any(|c| c.hit_id == Some(button)));
        let frame =
            apply_touch_events(&mut state, [(2, TouchPhase::Ended, inside)], controls).unwrap();
        assert_eq!(frame.mod_taps, [button]);
        assert!(frame.lifted.is_empty());
        assert_eq!(state.sample().len(), 1);
        assert_eq!(state.sample()[0].hit_id, Some(touch::JOYSTICK));

        apply_touch_events(&mut state, [(3, TouchPhase::Started, inside)], controls).unwrap();
        let outside =
            apply_touch_events(&mut state, [(3, TouchPhase::Ended, [0.75, 0.75])], controls)
                .unwrap();
        assert!(outside.mod_taps.is_empty() && outside.lifted.is_empty());
    }

    /// A finger held on a mod button is cancelled when the buttons change, while other fingers
    /// keep their controls.
    #[test]
    fn changed_mod_buttons_cancel_their_held_fingers() {
        let button = touch::mod_button(0).unwrap();
        let controls = |position: [f32; 2]| {
            let (hit_id, min, max) = if position[0] < 0.5 {
                (touch::JOYSTICK, [0.0, 0.0], [0.5, 1.0])
            } else {
                (button, [0.5, 0.0], [1.0, 1.0])
            };
            Some(TouchControlRegion {
                hit_id,
                bounds: TouchBounds { min, max },
            })
        };
        let inside = [0.75, 0.5];
        let mut state = TouchControlState::default();
        apply_touch_events(
            &mut state,
            [
                (1, TouchPhase::Started, [0.25, 0.5]),
                (2, TouchPhase::Started, inside),
            ],
            controls,
        )
        .unwrap();
        cancel_mod_button_contacts(&mut state);
        let held = state.sample();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].hit_id, Some(touch::JOYSTICK));
        let lifted =
            apply_touch_events(&mut state, [(2, TouchPhase::Ended, inside)], controls).unwrap();
        assert!(lifted.mod_taps.is_empty());
    }
}
