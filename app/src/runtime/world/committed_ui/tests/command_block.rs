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
