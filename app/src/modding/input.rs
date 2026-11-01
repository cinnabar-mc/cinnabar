//! Routes granted personal controls before the ordinary gameplay sample.

mod field;
mod physical;
use field::PanelSample;

use super::{MenuRuntime, ModRuntime};
use bevy::{
    ecs::{message::MessageCursor, system::SystemParam},
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::{
            AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButtonInput, MouseScrollUnit,
        },
    },
    prelude::*,
    window::{CursorOptions, Ime, PrimaryWindow, WindowEvent},
};
use client_shell::physical_input::set_cursor_capture;
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
#[cfg(test)]
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Physical messages and shared services routed while a personal panel owns input.
#[derive(SystemParam)]
pub(super) struct PanelInput<'w> {
    keyboard_events: Option<Res<'w, Messages<KeyboardInput>>>,
    ime_events: Option<Res<'w, Messages<Ime>>>,
    ordered_events: Option<Res<'w, Messages<WindowEvent>>>,
    clipboard: Option<ResMut<'w, client_shell::menu::MenuClipboard>>,
    mouse_events: Option<Res<'w, Messages<MouseButtonInput>>>,
    scroll: Option<Res<'w, AccumulatedMouseScroll>>,
    reserved: Option<ResMut<'w, super::ReservedControls>>,
}

pub(super) const POINTER_BUTTONS: [(MouseButton, &str); 3] = [
    (MouseButton::Left, "MouseLeft"),
    (MouseButton::Right, "MouseRight"),
    (MouseButton::Middle, "MouseMiddle"),
];
const TEXT_INPUT: &str = "TextInput";

#[derive(Default)]
pub(super) struct PhysicalControls {
    keys: MessageCursor<KeyboardInput>,
    mouse: MessageCursor<MouseButtonInput>,
    ime: MessageCursor<Ime>,
    ordered: MessageCursor<WindowEvent>,
    ime_owned: bool,
    left_held: bool,
    held_keys: Vec<String>,
    /// Whether `held_keys` was seeded from physical input since the mod attached or refocused.
    held_seeded: bool,
    restore_capture: bool,
    panel_owned: bool,
    /// A packet panel accepted this primary gesture; interruption requires a fresh press.
    panel_pointer_active: bool,
}

/// Applies native editor shortcuts without forwarding command letters or touching unfocused fields.
fn native_editor_shortcut(
    ui: &mut mod_ui::ModUi,
    clipboard: Option<&mut client_shell::menu::MenuClipboard>,
    key: &str,
) -> bool {
    if key == "KeyA" {
        ui.mod_panel_key("SelectAll", None);
        return true;
    }
    if !matches!(key, "KeyC" | "KeyX" | "KeyV") {
        return false;
    }
    let Some(selection) = ui.mod_panel_field_selection() else {
        return true;
    };
    let Some(clipboard) = clipboard else {
        return true;
    };
    if key == "KeyV" {
        let maximum = ui::mod_panel::MAX_PANEL_VALUE_CHARS * char::MAX.len_utf8();
        if let Some(text) = ui::ChatClipboard::read_text_bounded(clipboard, maximum)
            .ok()
            .flatten()
        {
            ui.mod_panel_key("Paste", Some(&text));
        }
    } else if !selection.is_empty() {
        clipboard.write_text(selection.to_owned());
        if key == "KeyX" {
            ui.mod_panel_key("Delete", None);
        }
    }
    true
}

#[allow(
    clippy::too_many_arguments,
    reason = "Routes one physical frame before gameplay authority."
)]
pub(super) fn prepare_mod_input(
    extension: Option<ResMut<ModRuntime>>,
    mut packets: Option<ResMut<super::packets::PacketMods>>,
    mut physical: Local<PhysicalControls>,
    input: PanelInput,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: Option<ResMut<ButtonInput<MouseButton>>>,
    mut motion: Option<ResMut<AccumulatedMouseMotion>>,
    mut windows: Query<(Entity, &mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    ui: Res<UiRuntimeResource>,
    player: Res<client_state::PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationResource>,
    time: Option<Res<Time>>,
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
) {
    let PanelInput {
        keyboard_events,
        ime_events,
        ordered_events,
        mut clipboard,
        mouse_events,
        scroll,
        mut reserved,
    } = input;
    if let Some(reserved) = reserved.as_mut() {
        reserved.0.clear();
    }
    let mut extension = extension.filter(|runtime| !runtime.suspended);
    if let Some(extension) = extension.as_deref_mut()
        && let Some(order) = extension.hosts.reconcile_panel_requests()
        && packets
            .as_ref()
            .and_then(|packets| packets.panel_open_order())
            .is_none_or(|packet| order > packet)
    {
        super::drive::present_frame_panel(extension, &mut presentation);
    }
    let viewport = windows
        .single()
        .ok()
        .map(|(_, window, _)| presentation.mod_panel_viewport([window.width(), window.height()]))
        .filter(|size| size.iter().all(|extent| *extent > 0.0));
    if let Some(packets) = packets.as_mut() {
        super::packet_panel::poll(
            packets,
            &mut presentation,
            viewport,
            extension.as_deref_mut(),
        );
    }
    let reactor_surface_available = packets
        .as_ref()
        .is_some_and(|packets| packets.panel_handle().is_some());
    let reactor_surface_focused = packets.as_ref().is_some_and(|packets| packets.panel.owned);
    if extension.is_none() && !reactor_surface_focused && !reactor_surface_available {
        physical.discard_pending(keyboard_events.as_deref(), mouse_events.as_deref());
        if let Some(events) = &ime_events {
            physical.ime.clear(events);
        }
        if let Some(events) = &ordered_events {
            physical.ordered.clear(events);
        }
        if !physical.panel_owned && !physical.ime_owned {
            return;
        }
    }
    let Ok((entity, mut window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let focused = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    if focused {
        let seed_pointer = !physical.held_seeded;
        physical.seed_held(keys.get_pressed());
        if seed_pointer && let Some(buttons) = mouse.as_ref() {
            for (button, name) in POINTER_BUTTONS {
                physical.track_held(
                    name.into(),
                    buttons.pressed(button) && !buttons.just_pressed(button),
                );
            }
        }
    }
    let mut panel_keys = Vec::new();
    for sample in field::samples(
        &mut physical,
        ordered_events.as_deref(),
        keyboard_events.as_deref(),
        ime_events.as_deref(),
        entity,
    ) {
        let field::RawSample::Key(event) = sample else {
            if let field::RawSample::Ime(event) = sample {
                panel_keys.push(PanelSample::Ime(event));
            }
            continue;
        };
        let identified = !matches!(event.key_code, KeyCode::Unidentified(_));
        if !identified {
            continue;
        }
        let name = format!("{:?}", event.key_code);
        let down = event.state == ButtonState::Pressed;
        physical.track_held(name.clone(), down);
        if down && focused {
            panel_keys.push(PanelSample::Key((
                name,
                event
                    .text
                    .as_deref()
                    .or_else(|| match &event.logical_key {
                        bevy::input::keyboard::Key::Character(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .map(str::to_owned),
                event.repeat,
                field::modifiers(&physical.held_keys),
            )));
        }
    }
    let pointer_pressed =
        physical.pointer_edges(mouse_events.as_deref(), mouse.as_deref(), entity, focused);
    if !focused {
        physical.left_held = false;
        physical.held_keys.clear();
        physical.held_seeded = false;
    }
    let primary_pressed = pointer_pressed.iter().any(|key| key == "MouseLeft");
    panel_keys.extend(
        pointer_pressed
            .iter()
            .cloned()
            .map(|key| PanelSample::Key((key, None, false, Default::default()))),
    );
    let raw_key_presses: Vec<_> = panel_keys
        .iter()
        .filter_map(PanelSample::key)
        .filter(|(key, _, repeat, _)| !repeat && key != TEXT_INPUT)
        .take(mod_api::MAX_CONTROL_KEYS)
        .map(|(key, text, _, _)| mod_api::panel_input::Key {
            key: key.clone(),
            text: text
                .as_ref()
                .filter(|text| text.len() <= 256 && !text.chars().any(char::is_control))
                .cloned(),
        })
        .collect();
    let absorbed = menu.as_deref().map_or_else(
        || ui.ui_focused(&player),
        |menu| presentation.base_absorbs_gameplay_input(&player, &ui, menu),
    );
    if reactor_surface_available
        && focused
        && !absorbed
        && let Some(packets) = packets.as_mut()
        && !reactor_surface_focused
        && !presentation.mod_ui().mod_panel_open()
    {
        packets.panel.pending.keys = panel_keys
            .iter()
            .filter_map(PanelSample::key)
            .filter(|(_, _, repeat, _)| !repeat)
            .take(mod_api::MAX_CONTROL_KEYS)
            .map(|(key, text, _, _)| mod_api::panel_input::Key {
                key: key.clone(),
                text: text
                    .as_ref()
                    .filter(|text| text.len() <= 256 && !text.chars().any(char::is_control))
                    .cloned(),
            })
            .collect();
        packets.panel.pending.held = physical.held_sample();
        packets.panel.pending.focused = focused;
    }
    if !reactor_surface_focused {
        let (_, cancelled) = physical.focused_pointer_sample(
            false,
            focused,
            absorbed,
            window.cursor_position().is_some(),
            primary_pressed,
        );
        if let Some(packets) = packets.as_mut() {
            packets.panel.pending.pointer_held = false;
            packets.panel.pending.pointer_cancelled |= cancelled;
            packets.panel.pending.shift_held = physical
                .shift_held(focused && !absorbed && !presentation.mod_ui().mod_panel_open());
        }
    }
    if extension.is_none()
        && !reactor_surface_focused
        && !physical.panel_owned
        && !physical.ime_owned
    {
        return;
    }
    if reactor_surface_focused {
        let mut packets = packets.expect("packet-owned panel requires its packet session");
        let was_open = physical.panel_owned;
        let mut open = presentation.mod_ui().mod_panel_open();
        let editing = open && presentation.mod_ui().mod_panel_editing();
        let capture = presentation.mod_ui().mod_panel_capture_key();
        let mut events = Vec::new();
        let raw_keys = if focused && !absorbed {
            raw_key_presses
        } else {
            Vec::new()
        };
        if !focused {
            presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
        }
        if absorbed {
            presentation.mod_ui_mut().cancel_mod_panel_edit();
            open = false;
        }
        for sample in panel_keys {
            let Some((key, text, _, modifiers)) =
                field::route_sample(sample, presentation.mod_ui_mut(), focused && !absorbed)
            else {
                continue;
            };
            if !open || !focused || absorbed {
                continue;
            }
            if editing {
                if (modifiers.control || modifiers.super_key)
                    && native_editor_shortcut(
                        presentation.mod_ui_mut(),
                        clipboard.as_deref_mut(),
                        &key,
                    )
                {
                    continue;
                }
                let text = if (modifiers.control || modifiers.super_key) && key != TEXT_INPUT {
                    Some("")
                } else {
                    text.as_deref()
                };
                events.extend(
                    presentation
                        .mod_ui_mut()
                        .mod_panel_field_key(&key, text, modifiers),
                );
            } else if key == "Escape" && !capture {
                open = false;
            }
        }
        field::sync_ime(
            &mut physical,
            &mut window,
            presentation.mod_ui_mut(),
            focused && !absorbed,
        );
        presentation.mod_ui_mut().set_mod_panel_open(open);
        if open && focused {
            if let Some(position) = window.cursor_position() {
                events.extend(presentation.mod_ui_mut().mod_panel_events(
                    position.to_array(),
                    primary_pressed,
                    physical.left_held,
                ));
                if let Some(scroll) = scroll.as_ref() {
                    presentation.mod_ui_mut().scroll_mod_panel(
                        position.to_array(),
                        f64::from(scroll.delta.y),
                        scroll.unit == MouseScrollUnit::Pixel,
                    );
                }
            } else {
                presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
            }
        }
        open = presentation.mod_ui().mod_panel_open();
        let (pointer_held, pointer_cancelled) = physical.focused_pointer_sample(
            open,
            focused,
            absorbed,
            window.cursor_position().is_some(),
            primary_pressed,
        );
        if !open {
            if let Some(handle) = packets.panel_handle() {
                handle.close();
            }
            packets.panel.owned = false;
        }
        packets.panel.pending = mod_api::panel_input::Input {
            open,
            focused: focused && !absorbed,
            pointer_held,
            pointer_cancelled,
            shift_held: physical.shift_held(focused),
            viewport,
            fields: packets
                .panel
                .owner
                .map(|owner| presentation.mod_ui_mut().take_mod_panel_field_events(owner))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|event| serde_json::to_value(event).ok())
                .take(ui::mod_panel::MAX_PANEL_EVENTS)
                .collect(),
            held: if focused && !absorbed {
                physical.held_sample()
            } else {
                Vec::new()
            },
            pointer: window
                .cursor_position()
                .filter(|_| open && focused && !absorbed)
                .and_then(|position| {
                    presentation.hud_gui_point(
                        position.to_array(),
                        [window.physical_width(), window.physical_height()],
                        window.scale_factor(),
                    )
                }),
            wheel: scroll
                .as_deref()
                .filter(|_| open && focused && !absorbed)
                .map_or(0.0, wheel_notches),
            events: events
                .into_iter()
                .take(ui::mod_panel::MAX_PANEL_EVENTS)
                .map(|event| mod_api::panel_input::Event {
                    id: event.id,
                    value: event.value,
                })
                .collect(),
            keys: raw_keys
                .into_iter()
                .take(mod_api::MAX_CONTROL_KEYS)
                .collect(),
        };
        if let Some(extension) = extension.as_mut() {
            extension.controls = mod_host::empty_controls();
            extension.controls.panel_open = open;
        }
        if was_open
            && !open
            && focused
            && !absorbed
            && let Some(focus) = focus.as_deref_mut()
        {
            focus.authorize_screen_return();
        }
        let restore = physical.finish_panel(
            open,
            focused,
            absorbed,
            crate::camera::mouse_input_active(&window, &cursor, focus.as_deref(), driven.is_some()),
        );
        finish_capture(
            open,
            was_open,
            restore,
            &mut keys,
            mouse.as_deref_mut(),
            motion.as_deref_mut(),
            &mut cursor,
        );
        return;
    }
    let Some(mut extension) = extension else {
        field::sync_ime(&mut physical, &mut window, presentation.mod_ui_mut(), false);
        let absorbed = menu.as_deref().map_or_else(
            || ui.ui_focused(&player),
            |menu| presentation.base_absorbs_gameplay_input(&player, &ui, menu),
        );
        let restore = physical.finish_panel(
            false,
            focused,
            absorbed,
            crate::camera::mouse_input_active(&window, &cursor, focus.as_deref(), driven.is_some()),
        );
        if let Some(mouse) = mouse.as_mut() {
            mouse.clear();
        }
        keys.reset(KeyCode::Escape);
        if restore {
            set_cursor_capture(&mut cursor, true);
            if let Some(motion) = motion.as_mut() {
                motion.delta = Vec2::ZERO;
            }
        }
        return;
    };
    let was_open = physical.panel_owned;
    let owner = extension.hosts.panel_owner();
    let mut open =
        extension.hosts.host(owner).panel_open() && presentation.mod_ui().mod_panel_open();
    let editing = open && presentation.mod_ui().mod_panel_editing();
    if !focused {
        presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
    }
    if absorbed {
        presentation.mod_ui_mut().cancel_mod_panel_edit();
    }
    let mut pressed = pointer_pressed;
    let mut events = Vec::new();
    for sample in panel_keys {
        let Some((key, text, repeat, modifiers)) =
            field::route_sample(sample, presentation.mod_ui_mut(), focused && !absorbed)
        else {
            continue;
        };
        if POINTER_BUTTONS.iter().any(|(_, name)| *name == key) {
            continue;
        }
        if open && focused && !absorbed && editing {
            if (modifiers.control || modifiers.super_key)
                && native_editor_shortcut(presentation.mod_ui_mut(), clipboard.as_deref_mut(), &key)
            {
                continue;
            }
            // An empty text value also suppresses the editor's physical digit fallback.
            let text = if (modifiers.control || modifiers.super_key) && key != TEXT_INPUT {
                Some("")
            } else {
                text.as_deref()
            };
            events.extend(
                presentation
                    .mod_ui_mut()
                    .mod_panel_field_key(&key, text, modifiers),
            );
        } else if key != TEXT_INPUT && !repeat {
            pressed.push(key);
        }
    }
    let editor_closed_panel = open && !presentation.mod_ui().mod_panel_open();
    open &= presentation.mod_ui().mod_panel_open();
    let close_requested = pressed.iter().any(|key| key == "Escape")
        && open
        && !extension
            .hosts
            .host(owner)
            .panel()
            .is_some_and(|panel| panel.capture_key);
    if !extension.hosts.host(owner).is_active() || (focused && (absorbed || close_requested)) {
        open = false;
    }
    extension.hosts.host_mut(owner).set_panel_open(open);
    presentation.mod_ui_mut().set_mod_panel_open(open);
    let before_pointer_open = open;
    let was_held = mouse
        .as_ref()
        .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
    if open && focused {
        if let Some(position) = window.cursor_position() {
            events.extend(presentation.mod_ui_mut().mod_panel_events(
                position.to_array(),
                was_held,
                physical.left_held,
            ));
            if let Some(scroll) = scroll.as_ref() {
                presentation.mod_ui_mut().scroll_mod_panel(
                    position.to_array(),
                    f64::from(scroll.delta.y),
                    scroll.unit == MouseScrollUnit::Pixel,
                );
            }
        } else if !physical.left_held {
            // A release outside the window must end capture before a later pointer re-entry.
            presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
        }
    }
    // Chat clicks go to the chat-action owner, which need not own the panel.
    extension.chat_events = presentation
        .take_chat_extension_actions()
        .into_iter()
        .take(ui::mod_panel::MAX_PANEL_EVENTS)
        .map(|id| mod_host::ControlEvent { id, value: 1. })
        .collect();
    let events = events
        .into_iter()
        .take(ui::mod_panel::MAX_PANEL_EVENTS)
        .map(|event| mod_host::ControlEvent {
            id: event.id,
            value: event.value,
        })
        .collect();
    open = presentation.mod_ui().mod_panel_open();
    let pointer_closed = before_pointer_open && !open;
    if !open
        && focused
        && !absorbed
        && extension.hosts.host(owner).is_active()
        && (close_requested || pointer_closed || editor_closed_panel)
        && let Some(focus) = focus.as_deref_mut()
    {
        focus.authorize_screen_return();
    }
    extension.hosts.host_mut(owner).set_panel_open(open);
    let restore = physical.finish_panel(
        open,
        focused,
        absorbed,
        crate::camera::mouse_input_active(&window, &cursor, focus.as_deref(), driven.is_some()),
    );
    extension.controls = mod_host::ControlFrame {
        seconds: time
            .as_ref()
            .map_or(0.0, |time| time.delta_secs().clamp(0.0, 1.0)),
        focused,
        gameplay: false,
        panel_open: open,
        keys_pressed: if focused {
            raw_key_presses
                .into_iter()
                .map(|key| key.key)
                .filter(|_| !absorbed)
                .take(mod_api::MAX_CONTROL_KEYS)
                .collect()
        } else {
            Vec::new()
        },
        keys_held: if focused && !absorbed {
            physical.held_sample()
        } else {
            Vec::new()
        },
        keys_admitted: Vec::new(),
        events,
        pointer: window
            .cursor_position()
            .filter(|_| open && focused && !absorbed)
            .and_then(|position| {
                presentation.hud_gui_point(
                    position.to_array(),
                    [window.physical_width(), window.physical_height()],
                    window.scale_factor(),
                )
            })
            .map(|[x, y]| (x, y)),
        wheel: scroll
            .as_deref()
            .filter(|_| open && focused && !absorbed)
            .map_or(0.0, wheel_notches),
    };
    field::sync_ime(
        &mut physical,
        &mut window,
        presentation.mod_ui_mut(),
        focused && !absorbed,
    );
    let text_events = presentation
        .mod_ui_mut()
        .take_mod_panel_text_events(extension.panel_ui_owner(owner));
    if focused
        && !absorbed
        && !text_events.is_empty()
        && let Err(error) = extension
            .hosts
            .host_mut(owner)
            .queue_text_events(text_events)
    {
        warn!(%error,"native mod text edit rejected");
    }
    let field_events = presentation
        .mod_ui_mut()
        .take_mod_panel_field_events(extension.panel_ui_owner(owner));
    if focused
        && !absorbed
        && !field_events.is_empty()
        && let Err(error) = extension
            .hosts
            .host_mut(owner)
            .queue_field_events(field_events)
    {
        warn!(%error, "native mod field input rejected");
    }
    finish_capture(
        open,
        was_open,
        restore,
        &mut keys,
        mouse.as_deref_mut(),
        motion.as_deref_mut(),
        &mut cursor,
    );
    if !open {
        let subscriptions = extension.hosts.reserved_keys();
        consume_reserved(
            &mut keys,
            mouse.as_deref_mut(),
            &subscriptions,
            reserved.as_deref_mut(),
        );
    }
}

/// Applies shared pointer capture and input consumption for every native personal panel.
/// One frame's wheel turn in notches, positive away from the player; a pixel-precise wheel
/// reports its pixels / 16.
fn wheel_notches(scroll: &AccumulatedMouseScroll) -> f32 {
    match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 16.0,
    }
}

fn finish_capture(
    open: bool,
    was_open: bool,
    restore: bool,
    keys: &mut ButtonInput<KeyCode>,
    mut mouse: Option<&mut ButtonInput<MouseButton>>,
    mut motion: Option<&mut AccumulatedMouseMotion>,
    cursor: &mut Mut<CursorOptions>,
) {
    if (open || was_open)
        && let Some(mouse) = mouse.as_mut()
    {
        mouse.clear();
    }
    if was_open && !open {
        keys.reset(KeyCode::Escape);
    }
    if open {
        client_shell::physical_input::release_cursor(cursor);
        keys.clear();
        if let Some(motion) = motion.as_mut() {
            motion.delta = Vec2::ZERO;
        }
    } else if restore {
        set_cursor_capture(cursor, true);
        if let Some(motion) = motion.as_mut() {
            motion.delta = Vec2::ZERO;
        }
        keys.reset(KeyCode::Escape);
    }
}

/// Removes reserved held state and edges from both gameplay input paths.
fn consume_reserved(
    keys: &mut ButtonInput<KeyCode>,
    mouse: Option<&mut ButtonInput<MouseButton>>,
    reserved: &[String],
    mut consumed: Option<&mut super::ReservedControls>,
) {
    let is_reserved = |name: &str| reserved.iter().any(|key| key == name);
    let consumed_keys: Vec<_> = keys
        .get_pressed()
        .chain(keys.get_just_pressed())
        .chain(keys.get_just_released())
        .copied()
        .filter(|key| {
            let name = format!("{key:?}");
            is_reserved(&name)
        })
        .collect();
    for key in consumed_keys {
        keys.reset(key);
    }
    if let Some(mouse) = mouse {
        for (button, name) in POINTER_BUTTONS {
            if is_reserved(name) {
                mouse.reset(button);
            }
        }
    }
    if let Some(consumed) = consumed.as_mut() {
        consumed.0.extend_from_slice(reserved);
    }
}

#[cfg(test)]
mod focus_tests;

#[cfg(test)]
mod tests;
