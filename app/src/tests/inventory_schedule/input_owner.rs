use super::*;
use crate::ui_runtime::{
    data_driven::drive_data_driven_input, drive_chat_ui_actions, drive_command_block,
    drive_server_form_input, drive_sign_editor, emotes::drive_emote_input,
    input_owner::decide_input_owner, mob_effects::drive_mob_effect_screen_input,
};

/// Every screen reads the frame's input only after the frame's owner is decided.
#[test]
fn production_schedule_decides_the_input_owner_before_any_screen_reads() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    let schedules = app.world().resource::<Schedules>();
    let graph = schedules
        .get(Update)
        .expect("production Update schedule")
        .graph();
    let decide = system_node(graph, decide_input_owner, "decide_input_owner");
    for reader in [
        system_node(graph, drive_sign_editor, "drive_sign_editor"),
        system_node(graph, drive_command_block, "drive_command_block"),
        system_node(graph, drive_server_form_input, "drive_server_form_input"),
        system_node(graph, drive_data_driven_input, "drive_data_driven_input"),
        system_node(graph, drive_emote_input, "drive_emote_input"),
        system_node(
            graph,
            drive_mob_effect_screen_input,
            "drive_mob_effect_screen_input",
        ),
        system_node(graph, drive_chat_ui_actions, "drive_chat_ui_actions"),
        system_node(
            graph,
            drive_chat_keyboard_input,
            "drive_chat_keyboard_input",
        ),
        system_node(
            graph,
            drive_inventory_ui_actions,
            "drive_inventory_ui_actions",
        ),
    ] {
        assert!(dependency_path_exists(graph, decide, reader), "{reader:?}");
    }
}
