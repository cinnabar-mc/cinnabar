//! App routing for controller-driven container screens.
//!
//! The retained [`ControllerInventoryInput`] owns the domain `ControllerSession`, the active
//! controller, meaningful-mouse detection and the effective virtual pointer. Navigation and
//! action semantics live in `client-ui`; this module only samples Bevy input, arbitrates owners
//! and dispatches the session's press edges through the existing semantic action path.
//!
//! Physical positions are a provisional Cinnabar choice, not measured Bedrock defaults: South
//! takes or places, West takes half or places one, North quick-transfers and East is Back, each
//! passed through the configured A/B and X/Y swap. The left stick moves the cursor and the D-pad
//! steps focus. Gather and drop have no physical binding until their defaults are captured.
//! A gamepad Inventory binding on one of those four face buttons is contextual while a
//! controller-driven container is open; on any other button it keeps toggling the screen.

use super::inventory::cancel_from_mouse_back;
use bevy::{
    ecs::entity::Entity,
    input::{
        gamepad::{Gamepad, GamepadAxis, GamepadButton},
        mouse::MouseButton,
    },
    math::Vec2,
    prelude::{ButtonInput, Local, Query, Res, ResMut, Resource, Single, Time, With},
    time::Real,
    window::{CursorOptions, PrimaryWindow, Window},
};
use client_shell::{
    menu::settings_options::control_bindings::gamepad_button,
    physical_input::TRANSLATED_GAMEPAD_BUTTONS,
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_ui::ui_runtime::{
    UiRuntime,
    controller_inventory::{
        actions::{ContainerControllerAction, ControllerActionOutcome},
        navigation::{self, Frame, Mode, Target},
        session::{ControllerSession, Input},
    },
    presentation::UiPresentationRuntime,
};
use json_ui::{HitRegion, RectOut};
use launcher::menu::settings_options::{BindingId, SettingsOptions};
use semantic_input::PhysicalControl;
use std::{sync::Arc, time::Duration};
use ui::UiPoint;
use ui_screens::container_actions::InventoryCellHit;

/// Provisional cursor and repeat policy; none of these are measured Bedrock values.
const SESSION_SETTINGS: navigation::Settings = navigation::Settings {
    cursor_units_per_second: 500.0,
    cursor_deadzone: 0.25,
    repeat_delay: Duration::from_millis(400),
    repeat_interval: Duration::from_millis(120),
};

/// Logical pixels the OS cursor must travel from its anchor before it takes the pointer back.
const MOUSE_TAKEOVER_DISTANCE: f32 = 3.0;

/// The physical position an action uses, or `None` while its default is uncaptured.
const fn action_button(action: ContainerControllerAction) -> Option<GamepadButton> {
    match action {
        ContainerControllerAction::Primary => Some(GamepadButton::South),
        ContainerControllerAction::HalfOrPlaceOne => Some(GamepadButton::West),
        ContainerControllerAction::QuickTransfer => Some(GamepadButton::North),
        ContainerControllerAction::Gather
        | ContainerControllerAction::DropOne
        | ContainerControllerAction::DropAll => None,
    }
}

/// Back's physical position.
const BACK_BUTTON: GamepadButton = GamepadButton::East;

/// The physical button that reads a role's position, after the saved A/B and X/Y swaps. The
/// swap permutes the face buttons, so each physical button carries at most one role.
pub(crate) fn role_button(options: Option<&SettingsOptions>, role: GamepadButton) -> GamepadButton {
    options.map_or(role, |options| gamepad_button(options, role))
}

/// Which pointer the inventory screen reads this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EffectivePointer {
    /// The OS cursor.
    Os,
    /// The controller's cursor in GUI coordinates; the OS cursor is stale and ignored.
    Virtual([f32; 2]),
}

/// Detects real mouse activity, ignoring a cursor that merely rests where it was.
#[derive(Debug, Default)]
struct MouseTracker {
    anchor: Option<Vec2>,
}

impl MouseTracker {
    /// The cursor position when it moved meaningfully since the last takeover or a button
    /// edge occurred; slow deliberate movement accumulates, jitter does not.
    fn observe(&mut self, position: Option<Vec2>, clicked: bool) -> Option<Vec2> {
        let Some(position) = position else {
            self.anchor = None;
            return None;
        };
        let Some(anchor) = self.anchor else {
            self.anchor = Some(position);
            return clicked.then_some(position);
        };
        if clicked || anchor.distance(position) >= MOUSE_TAKEOVER_DISTANCE {
            self.anchor = Some(position);
            return Some(position);
        }
        None
    }
}

/// Counts layouts, advancing only when the compared geometry actually changes.
#[derive(Debug)]
struct RevisionTracker<S> {
    revision: u64,
    last: Option<S>,
}

impl<S> Default for RevisionTracker<S> {
    fn default() -> Self {
        Self {
            revision: 0,
            last: None,
        }
    }
}

impl<S: PartialEq + Clone> RevisionTracker<S> {
    /// The monotonic revision of `snapshot`; an identical snapshot keeps its revision.
    fn observe(&mut self, snapshot: &S) -> u64 {
        if self.last.as_ref() != Some(snapshot) {
            self.last = Some(snapshot.clone());
            self.revision += 1;
        }
        self.revision
    }
}

/// Everything that identifies one laid-out container, compared exactly.
#[derive(Clone, PartialEq)]
struct GeometrySnapshot {
    bounds: RectOut,
    hits: Arc<[HitRegion]>,
    /// Semantic cell per region key; interned identities are the session's and excluded.
    cells: Vec<(String, InventoryCellHit)>,
}

/// One controller's normalized sample for the open container.
#[derive(Debug, Default, PartialEq)]
struct PadSample {
    /// Whether any control is held or deflected.
    active: bool,
    /// Held Up, Down, Left, Right.
    directions: [bool; 4],
    /// Left stick, +x right and +y down.
    axes: [f64; 2],
    /// Held logical actions in `ContainerControllerAction::ALL` order.
    actions: [bool; ContainerControllerAction::ALL.len()],
    /// A Back press edge this frame.
    back_edge: bool,
}

/// Whether any control is held or a stick is deflected past the deadzone.
fn pad_in_use(pad: &Gamepad) -> bool {
    let axis = |axis| f64::from(pad.get(axis).unwrap_or(0.0));
    let stick = |x, y| axis(x).hypot(axis(y)) > SESSION_SETTINGS.cursor_deadzone;
    pad.get_pressed().next().is_some()
        || stick(GamepadAxis::LeftStickX, GamepadAxis::LeftStickY)
        || stick(GamepadAxis::RightStickX, GamepadAxis::RightStickY)
}

/// Reads one pad. `map` applies the configured face-button swap to a physical position.
fn sample_pad(pad: &Gamepad, map: impl Fn(GamepadButton) -> GamepadButton) -> PadSample {
    let axis = |axis| f64::from(pad.get(axis).unwrap_or(0.0));
    let axes = [
        axis(GamepadAxis::LeftStickX),
        -axis(GamepadAxis::LeftStickY),
    ];
    PadSample {
        active: pad_in_use(pad),
        directions: [
            GamepadButton::DPadUp,
            GamepadButton::DPadDown,
            GamepadButton::DPadLeft,
            GamepadButton::DPadRight,
        ]
        .map(|button| pad.pressed(button)),
        axes,
        actions: ContainerControllerAction::ALL
            .map(|action| action_button(action).is_some_and(|button| pad.pressed(map(button)))),
        back_edge: pad.just_pressed(map(BACK_BUTTON)),
    }
}

/// The controller that owns the screen: the current one while it is held or in use, else the
/// lowest-numbered active pad, else the lowest-numbered connected pad (so a neutral sample can
/// arm). An idle owner yields to a pad that becomes active; that pad's first press is spent
/// on the required release.
fn select_owner(pads: &[(u64, bool)], current: Option<u64>) -> Option<u64> {
    let first = |active_only: bool| {
        pads.iter()
            .filter(|(_, active)| *active || !active_only)
            .map(|(id, _)| *id)
            .min()
    };
    let current = current.filter(|id| pads.iter().any(|(pad, _)| pad == id));
    match current {
        Some(id) if pads.iter().any(|(pad, active)| *pad == id && *active) => Some(id),
        Some(id) => first(true).or(Some(id)),
        None => first(true).or_else(|| first(false)),
    }
}

/// Lets Back act only after its controller was seen neutral within the same screen lifetime.
#[derive(Debug, Default)]
struct BackGate {
    scope: Option<(u64, u64)>,
    armed: bool,
}

impl BackGate {
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether a Back edge acts. `scope` is (container generation, controller); a change of
    /// either, or a mouse takeover, discards earlier neutrality, so a press that dismissed a
    /// modal or arrived with a replaced screen or pad cannot close this one.
    fn accept(&mut self, scope: (u64, u64), neutral: bool, edge: bool, mouse: bool) -> bool {
        if self.scope != Some(scope) || mouse {
            self.scope = Some(scope);
            self.armed = false;
        }
        let accepted = edge && self.armed && !mouse;
        self.armed |= neutral && !mouse;
        accepted
    }
}

/// Retained controller-inventory state; one per app.
#[derive(Resource)]
pub(crate) struct ControllerInventoryInput {
    session: ControllerSession,
    owner: Option<u64>,
    mouse: MouseTracker,
    /// The OS cursor position when it took the pointer this frame.
    takeover: Option<Vec2>,
    virtual_gui: Option<[f32; 2]>,
    revisions: RevisionTracker<GeometrySnapshot>,
    back: BackGate,
    /// Whether the container was open at frame start, and its generation then.
    start_open: bool,
    start_generation: u64,
    /// Whether any pad was in use this frame, sampled with the frame start.
    pad_in_use: bool,
    /// Latched for the frame: the UI owned this frame's controller edges. Never cleared by
    /// the drive system, so an earlier close or a close/reopen still retires them.
    release_gameplay: bool,
}

impl Default for ControllerInventoryInput {
    fn default() -> Self {
        Self {
            session: ControllerSession::new(SESSION_SETTINGS)
                .expect("controller session settings are valid"),
            owner: None,
            mouse: MouseTracker::default(),
            takeover: None,
            virtual_gui: None,
            revisions: RevisionTracker::default(),
            back: BackGate::default(),
            start_open: false,
            start_generation: 0,
            pad_in_use: false,
            release_gameplay: false,
        }
    }
}

impl ControllerInventoryInput {
    /// The pointer the screen reads now. A real mouse takeover this frame beats the virtual
    /// cursor; otherwise a stale OS cursor never overwrites or clears it.
    pub(crate) fn pointer(&self) -> EffectivePointer {
        match (self.takeover, self.virtual_gui) {
            (None, Some(gui)) => EffectivePointer::Virtual(gui),
            _ => EffectivePointer::Os,
        }
    }

    /// Whether this frame's controller edges belong to the container screen, so gameplay
    /// must neither see them nor act on them, even if the screen closes within the frame.
    pub(crate) fn consumed_gameplay_edges(&self) -> bool {
        self.release_gameplay
    }

    /// Latches UI ownership when the screen was or is open, or its lifetime changed since the
    /// frame start, while a pad is in use.
    fn latch_ui_owned(&mut self, open: bool, generation: u64) {
        if self.pad_in_use && (self.start_open || open || generation != self.start_generation) {
            self.release_gameplay = true;
        }
    }

    fn deactivate(&mut self, runtime: &mut UiRuntime) {
        self.session.reset();
        self.back.reset();
        if self.virtual_gui.take().is_some() {
            reset_pointer_gestures(runtime);
            runtime.set_inventory_pointer_gui(None);
        }
        runtime.set_inventory_controller_focus(None);
        runtime.set_inventory_controller_cursor_visible(false);
    }
}

/// The effective pointer for an optional controller resource.
pub(crate) fn effective_pointer(input: Option<&ControllerInventoryInput>) -> EffectivePointer {
    input.map_or(EffectivePointer::Os, ControllerInventoryInput::pointer)
}

/// Cancels pointer-driven gestures when the pointer owner changes, without dropping stacks.
fn reset_pointer_gestures(runtime: &mut UiRuntime) {
    let state = runtime.screen_state_mut();
    state.pointer.reset();
    state.container_close_armed = false;
    state.container_scroll_drag = None;
}

/// Whether controller navigation owns the open container.
pub(crate) fn owns_open_container(
    runtime: &UiRuntime,
    presentation: Option<&UiPresentationRuntime>,
) -> bool {
    runtime.inventory_open()
        && !runtime.screen_state().text_focused()
        && runtime.screen_state().book.is_none()
        && presentation.is_some_and(|view| {
            view.engine_container_frame().is_some()
                && !view.mod_ui().mod_view_shown()
                && !view.mod_ui().mod_text_focused()
                && !view.mod_ui().mod_panel_open()
        })
}

/// Whether the gamepad Inventory binding `control` is a physical button a container role reads.
/// Such a press is contextual action while the container is controller-driven, so it must not
/// also toggle the screen. Shoulders, thumbs, triggers, menu buttons and unbound controls read
/// no role, so their toggle keeps working.
pub(crate) fn inventory_control_reads_role(
    options: &SettingsOptions,
    control: Option<PhysicalControl>,
) -> bool {
    let Some(PhysicalControl::GamepadButton(code)) = control else {
        return false;
    };
    let Some((_, button)) = TRANSLATED_GAMEPAD_BUTTONS
        .iter()
        .find(|(candidate, _)| *candidate == code)
    else {
        return false;
    };
    ContainerControllerAction::ALL
        .into_iter()
        .filter_map(action_button)
        .chain(std::iter::once(BACK_BUTTON))
        .any(|role| role_button(Some(options), role) == *button)
}

/// Whether the gamepad Inventory binding is contextual on this open container.
pub(crate) fn inventory_binding_is_contextual(
    menu: Option<&client_shell::menu::MenuRuntime>,
    runtime: &UiRuntime,
    presentation: Option<&UiPresentationRuntime>,
) -> bool {
    if !owns_open_container(runtime, presentation) {
        return false;
    }
    let snapshot = menu.map(|menu| menu.settings_snapshot().0);
    let defaults = SettingsOptions::default();
    let options = snapshot.as_deref().unwrap_or(&defaults);
    let control = BindingId::Inventory
        .row(true)
        .and_then(|row| options.key_control(row));
    inventory_control_reads_role(options, control)
}

/// Records this frame's meaningful OS mouse activity before any earlier press routing.
pub(crate) fn sample_controller_pointer_ownership(
    mut input: ResMut<ControllerInventoryInput>,
    window: Single<&Window, With<PrimaryWindow>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    runtime: Res<UiRuntimeResource>,
    pads: Query<&Gamepad>,
) {
    input.start_open = runtime.inventory_open();
    input.start_generation = runtime.container_generation();
    input.pad_in_use = pads.iter().any(pad_in_use);
    input.release_gameplay = false;
    let clicked = mouse_buttons.get_just_pressed().next().is_some()
        || mouse_buttons.get_just_released().next().is_some();
    input.takeover = input.mouse.observe(window.cursor_position(), clicked);
}

/// Owns only the temporary visibility override for a virtual pointer. Restores it without
/// requiring the desktop camera policy, but never changes a captured gameplay cursor.
pub(crate) fn hide_os_cursor_for_controller(
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    runtime: Res<UiRuntimeResource>,
    input: Res<ControllerInventoryInput>,
    mut hidden_by_controller: Local<bool>,
) {
    let (window, mut cursor) = window.into_inner();
    if window.focused
        && runtime.inventory_open()
        && runtime.inventory_controller_cursor_visible()
        && matches!(input.pointer(), EffectivePointer::Virtual(_))
    {
        cursor.visible = false;
        *hidden_by_controller = true;
    } else if *hidden_by_controller && cursor.grab_mode == bevy::window::CursorGrabMode::None {
        cursor.visible = true;
        *hidden_by_controller = false;
    }
}

/// Steps the controller session for the open container and dispatches its action edges.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Bevy limits systems to 16 parameters, so related inputs share tuples."
)]
pub(crate) fn drive_controller_inventory(
    mut input: ResMut<ControllerInventoryInput>,
    mut player_runtime: ResMut<client_state::PlayerRuntime>,
    mut runtime: ResMut<UiRuntimeResource>,
    presentation: Res<UiPresentationResource>,
    pads: Query<(Entity, &Gamepad)>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    menu: Option<Res<client_shell::menu::MenuRuntime>>,
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    gates: (
        Option<Res<client_state::DrivenInput>>,
        Option<Res<client_state::ConsentInput>>,
        Option<Res<client_state::ScreenInputConsumed>>,
    ),
) {
    let (driven, consent, emote_input) = gates;
    let generation = runtime.container_generation();
    input.latch_ui_owned(runtime.inventory_open(), generation);
    if !runtime.inventory_open() {
        input.deactivate(&mut runtime);
        return;
    }
    let options = menu.as_deref().map(|menu| menu.settings_snapshot().0);
    let map = |button: GamepadButton| role_button(options.as_deref(), button);
    let samples: Vec<(u64, PadSample)> = pads
        .iter()
        .map(|(entity, pad)| (entity.to_bits(), sample_pad(pad, map)))
        .collect();
    let input_available = !consent.is_some_and(|consent| consent.0)
        && (driven.is_some()
            || (window.focused && focus.as_ref().is_none_or(|focus| focus.available())));
    let view = &presentation.0;
    let blocked = !input_available
        || runtime.credits().owns_input()
        || runtime.server_forms().owns_input()
        || runtime.data_driven_ui().owns_input()
        || runtime.sign_editor().is_open()
        || runtime.emotes().is_open()
        || emote_input.is_some_and(|consumed| consumed.0)
        || runtime.chat_focused()
        || menu.as_ref().is_some_and(|menu| menu.is_visible())
        || !owns_open_container(&runtime, Some(view));
    let (Some(engine), Some(bounds)) = (
        view.engine_container_frame(),
        view.inventory_gui_bounds(
            [window.physical_width(), window.physical_height()],
            window.scale_factor(),
        ),
    ) else {
        input.deactivate(&mut runtime);
        return;
    };
    let owner = select_owner(
        &samples
            .iter()
            .map(|(id, sample)| (*id, sample.active))
            .collect::<Vec<_>>(),
        input.owner,
    );
    input.owner = owner;
    let sample = owner.and_then(|owner| {
        samples
            .iter()
            .find(|(id, _)| *id == owner)
            .map(|(_, sample)| sample)
    });
    let Some(sample) = sample.filter(|_| !blocked) else {
        input.deactivate(&mut runtime);
        return;
    };
    let owner = owner.unwrap_or_default();

    // Only regions that take focus compete for a cell, so renderer children cannot shadow it.
    // The session interns identities, drops unsupported cells and their focus candidates.
    let cells: Vec<(String, InventoryCellHit)> = engine
        .hits
        .iter()
        .filter(|hit| hit.takes_focus())
        .filter_map(|hit| Some((hit.key.clone(), view.engine_container_region_hit(hit)?)))
        .collect();
    let targets: Vec<Target> = cells
        .iter()
        .map(|(key, hit)| Target {
            key: key.clone(),
            identity: 0,
            hit: *hit,
        })
        .collect();
    let revision = input.revisions.observe(&GeometrySnapshot {
        bounds,
        hits: Arc::clone(&engine.hits),
        cells,
    });
    let mouse = input.takeover.and_then(|point| {
        let point = UiPoint::new(point.x, point.y).ok()?;
        let gui = view.inventory_gui_point(
            point,
            [window.physical_width(), window.physical_height()],
            window.scale_factor(),
        )?;
        Some(gui.map(f64::from))
    });
    let output = input.session.step(
        &mut runtime,
        Frame {
            generation,
            geometry_revision: revision,
            bounds,
            hits: &engine.hits,
            targets: &targets,
        },
        Input {
            navigation: navigation::Input {
                now: time.elapsed(),
                controller: owner,
                active: true,
                directions: sample.directions,
                axes: sample.axes,
                mouse,
            },
            actions: sample.actions,
        },
    );
    let virtual_gui = matches!(output.navigation.mode, Mode::Focus | Mode::Cursor)
        .then_some(output.navigation.cursor)
        .flatten()
        .map(|point| point.map(|axis| axis as f32));
    if input.virtual_gui.is_none() != virtual_gui.is_none() {
        // A pointer-owner change cancels held mouse gestures; carried stacks stay put.
        reset_pointer_gestures(&mut runtime);
    }
    input.virtual_gui = virtual_gui;
    // Read before `input.back` is borrowed mutably through the resource.
    let mouse_takeover = input.takeover.is_some();
    let back = input.back.accept(
        (generation, owner),
        !sample.active,
        sample.back_edge,
        mouse_takeover,
    );
    if back {
        if cancel_from_mouse_back(&mut player_runtime, &mut runtime, Some(view))
            && !runtime.inventory_open()
        {
            if let Some(focus) = focus.as_deref_mut() {
                focus.authorize_screen_return();
            }
            input.deactivate(&mut runtime);
        }
        return;
    }
    let Some(selection) = &output.navigation.selection else {
        return;
    };
    for action in output.actions {
        let Some(screen) = runtime.container_screen_token(&player_runtime, selection.generation)
        else {
            break;
        };
        let outcome = runtime.dispatch_container_controller_action(
            &mut player_runtime,
            action,
            screen,
            selection,
            selection,
        );
        if !matches!(outcome, ControllerActionOutcome::Admitted { .. }) {
            bevy::log::debug!(
                ?action,
                ?outcome,
                "controller container action not admitted"
            );
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod integration_tests;
