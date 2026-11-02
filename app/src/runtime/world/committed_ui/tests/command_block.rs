use client_ui::ui_runtime::command_block::CommandBlockEdit;
use ui_screens::command_block_screen::CommandBlockMode;

use super::*;

fn change_dimension(app: &mut App, sequence: u64, dimension: i32) {
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            sequence,
            WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                dimension,
                position: [0.0, 70.0, 0.0],
                ..Default::default()
            }),
        )
        .unwrap();
}

/// An edit can't be saved against the block at the same coordinates in another dimension.
#[test]
fn a_committed_dimension_change_closes_the_open_command_block_editor() {
    let (mut app, _) = fixture_app();
    let mut schedule = Schedule::default();
    schedule.add_systems(drain_committed_ui_before_authority);
    schedule.run(app.world_mut());
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .command_block_mut()
        .open(CommandBlockEdit::load(
            [2, -60, 0],
            CommandBlockMode::Impulse,
            Some(false),
            &world::NbtCompound::default(),
            |_| None,
        ));
    schedule.run(app.world_mut());
    let is_open = |app: &App| {
        app.world()
            .resource::<UiRuntimeResource>()
            .command_block()
            .is_open()
    };
    assert!(is_open(&app), "the editor outlives an ordinary frame");

    change_dimension(&mut app, 1, 2);
    schedule.run(app.world_mut());
    assert!(!is_open(&app));
}

/// A form the server sends after opening a command block finds the editor asked for, whichever
/// frame it opens in, and is answered busy as it would be over the open editor.
#[test]
fn a_form_after_a_command_block_request_is_answered_busy() {
    let (mut app, _) = fixture_app();
    let mut schedule = Schedule::default();
    schedule.add_systems(drain_committed_ui_before_authority);
    schedule.run(app.world_mut());
    let mut world = app.world_mut().resource_mut::<ClientWorld>();
    let stream = world.stream.as_mut().unwrap();
    stream
        .submit(
            1,
            WorldEvent::OpenCommandBlock(protocol::OpenCommandBlockEvent {
                dimension: 0,
                position: [2, -60, 0],
            }),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::Ui(protocol::UiEvent::Form(protocol::FormRequestEvent {
                form_id: 3,
                kind: protocol::FormKind::Menu,
                title: None,
                json: "{}".into(),
                model: protocol::ServerFormModel::Modal(protocol::ModalDialogForm {
                    title: "Title".into(),
                    content: "Content".into(),
                    button1: "Yes".into(),
                    button2: "No".into(),
                }),
            })),
        )
        .unwrap();
    schedule.run(app.world_mut());
    let forms = app.world().resource::<UiRuntimeResource>();
    assert!(forms.server_forms().active().is_none());
    assert_eq!(forms.server_forms().queued_busy_count(), 1);
}

/// The coordinates of a request left waiting name another block after a transition.
#[test]
fn a_committed_dimension_change_drops_a_waiting_command_block_request() {
    let (mut app, _) = fixture_app();
    let mut schedule = Schedule::default();
    schedule.add_systems(drain_committed_ui_before_authority);
    schedule.run(app.world_mut());
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            1,
            WorldEvent::OpenCommandBlock(protocol::OpenCommandBlockEvent {
                dimension: 0,
                position: [2, -60, 0],
            }),
        )
        .unwrap();
    schedule.run(app.world_mut());
    let requested = |app: &App| {
        app.world()
            .resource::<UiRuntimeResource>()
            .command_block()
            .requested()
    };
    assert!(requested(&app));
    change_dimension(&mut app, 2, 2);
    schedule.run(app.world_mut());
    assert!(!requested(&app));
}
