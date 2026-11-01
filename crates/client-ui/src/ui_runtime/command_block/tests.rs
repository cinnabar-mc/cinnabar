use std::sync::Arc;

use json_ui::{ButtonEvent, MappingScope};
use world::{NbtCompound, NbtValue};

use super::*;

/// A compound as BDS sends a repeat block with a tracked last output.
fn actor() -> NbtCompound {
    let mut actor = NbtCompound::default();
    actor.insert("Command", NbtValue::String("say hi".into()));
    actor.insert("CustomName", NbtValue::String("note".into()));
    actor.insert("FilteredCustomName", NbtValue::String("n**e".into()));
    actor.insert("LastOutput", NbtValue::String("greet".into()));
    actor.insert(
        "LastOutputParams",
        NbtValue::List(vec![NbtValue::String("Steve".into())]),
    );
    actor.insert("TickDelay", NbtValue::Int(5));
    actor.insert("TrackOutput", NbtValue::Byte(1));
    actor.insert("ExecuteOnFirstTick", NbtValue::Byte(0));
    actor.insert("auto", NbtValue::Byte(1));
    actor.insert("LPCommandMode", NbtValue::Int(2));
    actor.insert("LPCondionalMode", NbtValue::Byte(1));
    actor.insert("LPRedstoneMode", NbtValue::Byte(1));
    actor
}

fn loaded() -> CommandBlockEdit {
    CommandBlockEdit::load(
        [2, -60, 0],
        CommandBlockMode::Repeat,
        Some(true),
        &actor(),
        |key| (key == "greet").then(|| Arc::from("Hello %s")),
    )
}

fn editor() -> CommandBlockEditor {
    let mut editor = CommandBlockEditor::default();
    editor.open(loaded());
    editor
}

/// A frame with no controls, for events that name theirs.
fn frame() -> EngineFrame {
    EngineFrame {
        hits: Arc::from([]),
        report: Default::default(),
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    }
}

fn pressed(id: &str) -> ScreenEvent {
    ScreenEvent::Button(ButtonEvent {
        id: id.to_owned(),
        from: "button.menu_cancel".to_owned(),
        key: String::new(),
        collection_index: None,
        collection: None,
        down: true,
        interacted: true,
        scope: MappingScope::Global,
    })
}

fn toggled(name: &str, checked: bool) -> ScreenEvent {
    ScreenEvent::Toggle {
        name: name.to_owned(),
        key: name.to_owned(),
        index: None,
        checked,
        by_click: true,
    }
}

fn edited(name: &str, text: &str, finished: bool) -> ScreenEvent {
    ScreenEvent::TextEdit {
        name: name.to_owned(),
        key: name.to_owned(),
        index: None,
        text: text.to_owned(),
        finished,
    }
}

/// Applies `events` and closes, returning what the server would receive.
fn sent(editor: &mut CommandBlockEditor, events: &[ScreenEvent]) -> Option<protocol::Packet> {
    for event in events {
        editor.apply(&frame(), event, None);
    }
    let mut packet = None;
    assert!(editor.finish(|sent| {
        packet = Some(sent);
        Ok(())
    }));
    assert!(!editor.is_open());
    packet
}

#[test]
fn a_block_loads_its_state_from_the_block_and_its_actor() {
    let edit = loaded();
    let input = edit.input();
    assert_eq!(input.command, "say hi");
    assert_eq!(input.hover_note, "note");
    assert_eq!(input.tick_delay, 5);
    assert_eq!(input.last_output, "Hello Steve");
    assert_eq!(input.mode, CommandBlockMode::Repeat);
    assert!(input.conditional);
    // `auto` set means the block runs without redstone.
    assert!(!input.needs_redstone);
    assert!(input.track_output);
    assert!(input.shows_previous);
    assert_eq!(
        input.performed,
        PerformedModes {
            mode: CommandBlockMode::Chain,
            conditional: true,
            needs_redstone: true,
        }
    );
}

#[test]
fn a_block_without_stored_flags_needs_redstone_and_tracks_output() {
    let edit = CommandBlockEdit::load(
        [0, 0, 0],
        CommandBlockMode::Impulse,
        None,
        &NbtCompound::default(),
        |_| None,
    );
    assert!(edit.input().needs_redstone);
    assert!(edit.input().track_output);
    assert!(!edit.input().conditional);
}

#[test]
fn registry_state_names_the_conditional_bit() {
    assert_eq!(
        conditional_bit(r#"{"conditional_bit":{"type":"byte","value":1}}"#),
        Some(true)
    );
    assert_eq!(conditional_bit(r#"{"conditional_bit":0}"#), Some(false));
    assert_eq!(conditional_bit(r#"{"facing_direction":0}"#), None);
    assert_eq!(conditional_bit("not json"), None);
}

#[test]
fn block_names_choose_the_mode() {
    assert_eq!(
        mode_of_block(Some("minecraft:repeating_command_block")),
        CommandBlockMode::Repeat
    );
    assert_eq!(
        mode_of_block(Some("minecraft:chain_command_block")),
        CommandBlockMode::Chain
    );
    assert_eq!(
        mode_of_block(Some("minecraft:command_block")),
        CommandBlockMode::Impulse
    );
}

#[test]
fn closing_untouched_sends_nothing() {
    let mut editor = editor();
    assert!(sent(&mut editor, &[pressed("button.menu_exit")]).is_none());
}

#[test]
fn exit_reports_the_edits() {
    let mut editor = editor();
    let packet = sent(
        &mut editor,
        &[
            edited(COMMAND_TEXT_BOX, "say bye", false),
            edited(HOVER_NOTE_BOX, "renamed", false),
            edited(TICK_DELAY_BOX, "20", true),
            toggled("track_output", false),
            toggled("#command_chain_mode", true),
            toggled("#command_unconditional_mode", true),
            toggled("#command_needs_redstone_mode", true),
            pressed("button.menu_exit"),
        ],
    )
    .expect("a changed block reports");
    let expected = protocol::command_block_update_packet(&CommandBlockFields {
        position: [2, -60, 0],
        mode: 2,
        needs_redstone: true,
        conditional: false,
        command: "say bye".to_owned(),
        last_output: "Hello Steve".to_owned(),
        name: "renamed".to_owned(),
        // The server filters the new name.
        filtered_name: String::new(),
        track_output: false,
        tick_delay: 20,
        execute_on_first_tick: false,
    });
    assert_eq!(format!("{packet:?}"), format!("{expected:?}"));
}

#[test]
fn cancel_after_an_edit_reports_the_untouched_block() {
    let mut editor = editor();
    let packet = sent(
        &mut editor,
        &[
            edited(COMMAND_TEXT_BOX, "say bye", false),
            pressed("button.menu_leave_screen"),
        ],
    )
    .expect("a dirty screen reports on close");
    let expected = protocol::command_block_update_packet(&loaded().original);
    assert_eq!(format!("{packet:?}"), format!("{expected:?}"));
}

#[test]
fn choosing_a_mode_sets_execute_on_first_tick() {
    let mut editor = editor();
    editor.apply(&frame(), &toggled("#command_impulse_mode", true), None);
    assert!(!editor.active().unwrap().input().execute_on_first_tick);
    editor.apply(&frame(), &toggled("#command_repeat_mode", true), None);
    assert!(editor.active().unwrap().input().execute_on_first_tick);
    // A radio entry losing the group's selection changes nothing.
    editor.apply(&frame(), &toggled("#command_chain_mode", false), None);
    assert_eq!(
        editor.active().unwrap().input().mode,
        CommandBlockMode::Repeat
    );
}

#[test]
fn a_chosen_entry_collapses_its_dropdown() {
    let mut editor = editor();
    editor.apply(&frame(), &toggled("block_type_dropdown", true), None);
    assert!(editor.active().unwrap().input().open.block_type);
    editor.apply(&frame(), &toggled("#command_impulse_mode", true), None);
    assert!(!editor.active().unwrap().input().open.block_type);

    editor.apply(&frame(), &toggled("condition_dropdown", true), None);
    editor.apply(&frame(), &pressed("button.dropdown_exit"), None);
    assert!(!editor.active().unwrap().input().open.condition);
}

#[test]
fn a_tick_delay_applies_when_its_box_is_left() {
    let mut editor = editor();
    editor.apply(&frame(), &edited(TICK_DELAY_BOX, "42", false), None);
    assert_eq!(editor.active().unwrap().input().tick_delay, 5);
    editor.apply(&frame(), &edited(TICK_DELAY_BOX, "42", true), None);
    assert_eq!(editor.active().unwrap().input().tick_delay, 42);
    // What doesn't parse is zero.
    editor.apply(&frame(), &edited(TICK_DELAY_BOX, "4x", true), None);
    assert_eq!(editor.active().unwrap().input().tick_delay, 0);
}

#[test]
fn a_saved_tick_delay_stays_in_the_blocks_range() {
    let mut edit = loaded();
    edit.set(|fields| &mut fields.tick_delay, -3);
    assert_eq!(edit.closing_update(Exit::Save).unwrap().tick_delay, 0);
    edit.set(|fields| &mut fields.tick_delay, 123_456);
    assert_eq!(
        edit.closing_update(Exit::Save).unwrap().tick_delay,
        MAX_TICK_DELAY
    );
}

#[test]
fn turning_tracking_off_hides_the_previous_output() {
    let mut editor = editor();
    editor.apply(&frame(), &toggled("track_output", false), None);
    assert!(!editor.active().unwrap().input().shows_previous);
    editor.apply(&frame(), &toggled("track_output", true), None);
    assert!(editor.active().unwrap().input().shows_previous);
}

#[test]
fn a_refused_update_keeps_the_screen_open_to_retry() {
    let mut editor = editor();
    editor.apply(&frame(), &edited(COMMAND_TEXT_BOX, "say bye", false), None);
    editor.apply(&frame(), &pressed("button.menu_exit"), None);
    assert!(!editor.finish(|_| Err(())));
    assert!(editor.is_open());
    assert!(editor.finish(|_| Ok(())));
    assert!(!editor.is_open());
}

#[test]
fn maximizing_swaps_the_command_box_without_dirtying_the_block() {
    let mut editor = editor();
    editor.apply(&frame(), &pressed("command_block.input_maximize"), None);
    assert!(editor.active().unwrap().input().maximized);
    editor.apply(&frame(), &pressed("command_block.input_minimize"), None);
    assert!(!editor.active().unwrap().input().maximized);
    assert!(sent(&mut editor, &[pressed("button.menu_exit")]).is_none());
}
