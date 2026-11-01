//! Command block editor state: what the screen shows, loaded from the block actor, what the player
//! changes, and the update the server receives when the editor closes.

use std::sync::Arc;

use assets::facts::presentation::CommandBlockKind;
use json_ui::{ButtonEvent, HitKind, ScreenEvent};
use protocol::CommandBlockFields;
use ui::{ChatMessage, ChatMessageKind};
use ui_engine::frame::EngineFrame;
use ui_screens::command_block_screen::{
    CommandBlockInput, CommandBlockMode, OpenDropdowns, PerformedModes,
};
use world::{NbtCompound, NbtValue};

use super::forms::FormEngineState;
use super::forms::engine_input::{self, EngineInput};

/// The largest tick delay a command block stores.
const MAX_TICK_DELAY: i32 = 99_999;

/// The name of both command edit boxes; maximizing swaps which one is drawn.
const COMMAND_TEXT_BOX: &str = "command_block.command_text";
const HOVER_NOTE_BOX: &str = "command_block.hover_note_textbox";
const TICK_DELAY_BOX: &str = "command_block.tick_delay_textbox";

/// How the screen closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exit {
    /// Exit: the edits take effect.
    Save,
    /// Cancel: the edits are dropped.
    Leave,
}

/// One command block being edited.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandBlockEdit {
    /// The block as it is on the server. A leave reports this, because the screen's edits
    /// never reached the block.
    original: CommandBlockFields,
    /// The block as the screen shows it.
    current: CommandBlockFields,
    performed: PerformedModes,
    tracked_at_open: bool,
    /// Any setter changed a value, which alone makes the closing screen report to the server.
    dirty: bool,
    maximized: bool,
    open: OpenDropdowns,
}

impl CommandBlockEdit {
    /// Loads the block at `position` as vanilla's screen does: mode and conditional from the block
    /// itself, the rest from its actor. `translate` resolves the stored last output's language key.
    #[must_use]
    pub fn load(
        position: [i32; 3],
        mode: CommandBlockMode,
        conditional: Option<bool>,
        actor: &NbtCompound,
        translate: impl Fn(&str) -> Option<Arc<str>>,
    ) -> Self {
        let flag =
            |key: &str, default: bool| actor.integer(key).map_or(default, |value| value != 0);
        let text = |key: &str| actor.string(key).unwrap_or_default().to_owned();
        let conditional = conditional.unwrap_or_else(|| flag("conditionalMode", false));
        let current = CommandBlockFields {
            position,
            mode: mode.wire_value(),
            // The actor stores the inverse: `auto` blocks run without a signal.
            needs_redstone: !flag("auto", false),
            conditional,
            command: text("Command"),
            last_output: last_output_text(actor, translate),
            name: text("CustomName"),
            filtered_name: text("FilteredCustomName"),
            track_output: flag("TrackOutput", true),
            tick_delay: actor
                .integer("TickDelay")
                .and_then(|value| i32::try_from(value).ok())
                .unwrap_or(0),
            execute_on_first_tick: flag("ExecuteOnFirstTick", false),
        };
        Self {
            performed: PerformedModes {
                mode: CommandBlockMode::from_wire(actor.integer("LPCommandMode").unwrap_or(0)),
                conditional: flag("LPCondionalMode", false),
                needs_redstone: flag("LPRedstoneMode", false),
            },
            tracked_at_open: current.track_output,
            original: current.clone(),
            current,
            dirty: false,
            maximized: false,
            open: OpenDropdowns::default(),
        }
    }

    /// What the screen paints.
    #[must_use]
    pub fn input(&self) -> CommandBlockInput<'_> {
        CommandBlockInput {
            command: &self.current.command,
            hover_note: &self.current.name,
            tick_delay: self.current.tick_delay,
            last_output: &self.current.last_output,
            mode: CommandBlockMode::from_wire(i64::from(self.current.mode)),
            conditional: self.current.conditional,
            needs_redstone: self.current.needs_redstone,
            track_output: self.current.track_output,
            execute_on_first_tick: self.current.execute_on_first_tick,
            performed: self.performed,
            shows_previous: self.tracked_at_open && self.current.track_output,
            maximized: self.maximized,
            open: self.open,
        }
    }

    /// Applies `change`, marking the edit dirty when it moved a value.
    fn set<T: PartialEq>(&mut self, field: impl Fn(&mut CommandBlockFields) -> &mut T, value: T) {
        let slot = field(&mut self.current);
        if *slot != value {
            *slot = value;
            self.dirty = true;
        }
    }

    /// Choosing a mode also sets execute-on-first-tick: on for repeat, off otherwise.
    fn set_mode(&mut self, mode: CommandBlockMode) {
        self.set(|fields| &mut fields.mode, mode.wire_value());
        self.set(
            |fields| &mut fields.execute_on_first_tick,
            mode == CommandBlockMode::Repeat,
        );
    }

    /// A changed hover note has no filtered form until the server produces one.
    fn set_hover_note(&mut self, note: &str) {
        self.set(|fields| &mut fields.name, note.to_owned());
        if self.current.name != self.original.name {
            self.current.filtered_name.clear();
        }
    }

    /// What the server receives for a close, `None` when nothing changed. A save stores the
    /// delay within the block's range; a leave reports the untouched block.
    fn closing_update(&self, exit: Exit) -> Option<CommandBlockFields> {
        if !self.dirty {
            return None;
        }
        Some(match exit {
            Exit::Save => {
                let mut saved = self.current.clone();
                saved.tick_delay = saved.tick_delay.clamp(0, MAX_TICK_DELAY);
                saved
            }
            Exit::Leave => self.original.clone(),
        })
    }
}

/// The block's last output in the player's language, from its language key and parameters.
fn last_output_text(actor: &NbtCompound, translate: impl Fn(&str) -> Option<Arc<str>>) -> String {
    let key = actor.string("LastOutput").unwrap_or_default();
    let parameters: Arc<[Arc<str>]> = match actor.get("LastOutputParams") {
        Some(NbtValue::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                NbtValue::String(text) => Some(Arc::from(&**text)),
                _ => None,
            })
            .collect(),
        _ => Arc::from([]),
    };
    let row = ChatMessage {
        fifo_sequence: 0,
        received_millis: 0,
        kind: ChatMessageKind::Translation,
        source: None,
        message: Arc::from(key),
        parameters,
    };
    ui_screens::text::resolve_chat_line(&row, translate).into_owned()
}

/// A block's integer `conditional_bit` in the registry's canonical state JSON.
#[must_use]
pub fn conditional_bit(canonical_state: &str) -> Option<bool> {
    let state: serde_json::Value = serde_json::from_str(canonical_state).ok()?;
    let bit = state.get("conditional_bit")?;
    Some(bit.get("value").unwrap_or(bit).as_u64()? != 0)
}

/// The mode of the block named `name`.
#[must_use]
pub fn mode_of_block(name: Option<&str>) -> CommandBlockMode {
    match assets::facts::presentation::command_block_kind(name) {
        CommandBlockKind::Repeating => CommandBlockMode::Repeat,
        CommandBlockKind::Chain => CommandBlockMode::Chain,
        CommandBlockKind::Impulse => CommandBlockMode::Impulse,
    }
}

/// The open editor, its screen's input state, and how it was asked to close.
#[derive(Clone, Debug, Default)]
pub struct CommandBlockEditor {
    active: Option<CommandBlockEdit>,
    engine: FormEngineState,
    exit: Option<Exit>,
}

impl CommandBlockEditor {
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.active.is_some()
    }

    #[must_use]
    pub const fn active(&self) -> Option<&CommandBlockEdit> {
        self.active.as_ref()
    }

    #[must_use]
    pub const fn engine(&self) -> &FormEngineState {
        &self.engine
    }

    /// Shows `edit` on a fresh screen.
    pub fn open(&mut self, edit: CommandBlockEdit) {
        self.close();
        self.active = Some(edit);
    }

    /// Closes the screen without reporting to the server.
    pub fn close(&mut self) {
        self.active = None;
        self.exit = None;
        self.engine = FormEngineState::default();
    }

    /// Drops pointer input in progress, for a frame the editor can't read input.
    pub fn cancel_pointer_input(&mut self) {
        self.engine.cancel_pointer_input();
    }

    /// Routes one frame of input to the screen drawn as `frame`.
    pub fn drive(&mut self, frame: &EngineFrame, input: EngineInput<'_>) {
        if self.active.is_none() {
            return;
        }
        let dispatched = engine_input::dispatch(&mut self.engine, frame, input);
        for (at, event) in dispatched.events.iter().enumerate() {
            self.apply(frame, event, dispatched.pressed_at(at));
        }
    }

    /// Whether a button asked the screen to close.
    #[must_use]
    pub const fn exit_requested(&self) -> bool {
        self.exit.is_some()
    }

    /// Closes a screen asked to close once `send` accepted its update, if it has one. A refused
    /// update leaves it open to retry, so a transport hiccup loses no edit. `true` once closed.
    pub fn finish(&mut self, send: impl FnOnce(protocol::Packet) -> Result<(), ()>) -> bool {
        let Some(exit) = self.exit else {
            return false;
        };
        if let Some(fields) = self
            .active
            .as_ref()
            .and_then(|edit| edit.closing_update(exit))
            && send(protocol::command_block_update_packet(&fields)).is_err()
        {
            return false;
        }
        self.close();
        true
    }

    /// The screen controller: what one screen event does to the edit.
    fn apply(&mut self, frame: &EngineFrame, event: &ScreenEvent, pressed: Option<&str>) {
        match event {
            ScreenEvent::Button(button) => self.apply_button(frame, button, pressed),
            ScreenEvent::Toggle { name, checked, .. } => self.apply_toggle(frame, name, *checked),
            ScreenEvent::TextEdit {
                name,
                text,
                finished,
                ..
            } => self.apply_text(frame, name, text, *finished),
            ScreenEvent::Sound {
                name,
                volume,
                pitch,
            } => crate::sound_requests::ui_sound(name, *volume, *pitch),
            ScreenEvent::Slider { .. } | ScreenEvent::TextEditSelected { .. } => {}
        }
    }

    fn apply_button(&mut self, frame: &EngineFrame, button: &ButtonEvent, pressed: Option<&str>) {
        if button.id == "button.dropdown_exit" && button.down {
            if let Some(edit) = self.active.as_mut() {
                edit.open = OpenDropdowns::default();
            }
            self.forget_toggles(frame);
        }
        if !engine_input::activates(button, pressed) {
            return;
        }
        match button.id.as_str() {
            "button.menu_exit" => self.exit = Some(Exit::Save),
            "button.menu_leave_screen" => self.exit = Some(Exit::Leave),
            "command_block.input_maximize" => self.set_maximized(frame, true),
            "command_block.input_minimize" => self.set_maximized(frame, false),
            _ => {}
        }
    }

    /// Swaps which command box is drawn; each binds the current command when it next appears.
    fn set_maximized(&mut self, frame: &EngineFrame, maximized: bool) {
        let Some(edit) = self.active.as_mut() else {
            return;
        };
        edit.maximized = maximized;
        edit.open = OpenDropdowns::default();
        self.engine
            .dispatcher
            .clear_selection(&frame.hits, &mut self.engine.view);
        for region in frame.hits.iter() {
            if region.kind == HitKind::EditBox
                && region
                    .widget
                    .edit
                    .as_ref()
                    .is_some_and(|edit| edit.name.as_deref() == Some(COMMAND_TEXT_BOX))
            {
                self.engine.view.components.forget(&region.key);
            }
        }
    }

    fn apply_toggle(&mut self, frame: &EngineFrame, name: &str, checked: bool) {
        let Some(edit) = self.active.as_mut() else {
            return;
        };
        let mode = |edit: &mut CommandBlockEdit, mode| {
            if checked {
                edit.set_mode(mode);
            }
            checked
        };
        // A chosen radio entry collapses its dropdown.
        let collapses = match name {
            "block_type_dropdown" => {
                edit.open.block_type = checked;
                false
            }
            "condition_dropdown" => {
                edit.open.condition = checked;
                false
            }
            "redstone_dropdown" => {
                edit.open.redstone = checked;
                false
            }
            "#command_impulse_mode" => mode(edit, CommandBlockMode::Impulse),
            "#command_repeat_mode" => mode(edit, CommandBlockMode::Repeat),
            "#command_chain_mode" => mode(edit, CommandBlockMode::Chain),
            "#command_conditional_mode" | "#command_unconditional_mode" => {
                if checked {
                    let conditional = name == "#command_conditional_mode";
                    edit.set(|fields| &mut fields.conditional, conditional);
                }
                checked
            }
            "#command_needs_redstone_mode" | "#command_always_on_mode" => {
                if checked {
                    let needs_redstone = name == "#command_needs_redstone_mode";
                    edit.set(|fields| &mut fields.needs_redstone, needs_redstone);
                }
                checked
            }
            "track_output" => {
                edit.set(|fields| &mut fields.track_output, checked);
                false
            }
            "execute_on_first_tick" => {
                edit.set(|fields| &mut fields.execute_on_first_tick, checked);
                false
            }
            _ => return,
        };
        if collapses {
            edit.open = OpenDropdowns::default();
        }
        self.forget_toggles(frame);
    }

    /// Hands every toggle back to its binding, the edit being what the toggles show.
    fn forget_toggles(&mut self, frame: &EngineFrame) {
        for region in frame.hits.iter() {
            if matches!(region.kind, HitKind::Toggle | HitKind::Dropdown) {
                self.engine.view.components.forget(&region.key);
            }
        }
    }

    fn apply_text(&mut self, frame: &EngineFrame, name: &str, text: &str, finished: bool) {
        let Some(edit) = self.active.as_mut() else {
            return;
        };
        match name {
            COMMAND_TEXT_BOX => edit.set(|fields| &mut fields.command, text.to_owned()),
            HOVER_NOTE_BOX => edit.set_hover_note(text),
            // The delay takes effect once the box is left; what doesn't parse becomes zero,
            // which the box then shows.
            TICK_DELAY_BOX if finished => {
                let delay = text.trim().parse().unwrap_or(0);
                edit.set(|fields| &mut fields.tick_delay, delay);
                let shown = delay.to_string();
                self.engine.dispatcher.set_edit_text(
                    &frame.hits,
                    &mut self.engine.view,
                    TICK_DELAY_BOX,
                    &shown,
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
