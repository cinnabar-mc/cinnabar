//! The Video-section chat placement and input preferences use the existing dropdown templates.

use json_ui::{Catalog, Context};

use launcher::menu::settings_options::{
    CHAT_INPUT_OPTION, CHAT_POSITION_OPTION, SettingDefinition, SettingKind,
    chat::{CHAT_INPUTS, CHAT_POSITIONS},
};

/// Adds the chat preferences to the top of the vanilla Video section, each from its one shared
/// option definition.
pub fn install(catalog: &mut Catalog) {
    let options = [CHAT_POSITION_OPTION, CHAT_INPUT_OPTION];
    let mut overlay = serde_json::json!({
        "namespace": "general_section",
        "video_section": {
            "modifications": [{
                "array_name": "controls", "operation": "insert_front",
                "value": options.map(dropdown)
            }]
        }
    });
    for option in options {
        overlay[format!(
            "{}_content@settings_common.option_radio_dropdown_group",
            option.name
        )] = serde_json::json!({ "$radio_buttons": radio_rows(option) });
    }
    catalog.overlay_text("ui/cinnabar_chat_position.json", &overlay.to_string());
}

/// The Video-section dropdown row for `option`, whose radio group is `{name}_content`.
fn dropdown(option: SettingDefinition) -> serde_json::Value {
    let name = option.name;
    serde_json::json!({
        format!("{name}@settings_common.option_dropdown"): {
            "$option_label": option.label,
            "$dropdown_content": format!("general_section.{name}_content"),
            "$dropdown_area": "content_area",
            "$dropdown_name": format!("{name}_dropdown"),
            "$option_enabled_binding_name": format!("#{name}_dropdown_enabled"),
            "$options_dropdown_toggle_label_binding": format!("#{name}_dropdown_toggle_label"),
            "$dropdown_scroll_content_size": ["100%", "145%"]
        }
    })
}

/// One radio row per choice of the dropdown `option`, bound to the choice's toggle.
fn radio_rows(option: SettingDefinition) -> Vec<serde_json::Value> {
    let SettingKind::Dropdown(choices) = option.kind else {
        unreachable!("chat preferences are dropdowns")
    };
    choices
        .iter()
        .map(|choice| {
            serde_json::json!({
                "@settings_common.radio_with_label": {
                    "$toggle_state_binding_name": format!("#{}", choice.name),
                    "$radio_label_text": choice.label
                }
            })
        })
        .collect()
}

/// `context` with the chat at the top or bottom of the screen.
pub fn placed(context: Context, top: bool) -> Context {
    context.with_flag(CHAT_POSITIONS[1].name, top)
}

/// `context` with the open chat drawn as vanilla Bedrock's full chat screen rather than Java's
/// input line.
pub fn with_input(context: Context, bedrock: bool) -> Context {
    context.with_flag(CHAT_INPUTS[1].name, bedrock)
}
