//! Optional distant terrain and the radius drawn in full detail in the in-game Video screen.

use json_ui::Catalog;
use launcher::menu::settings_options::SettingId;
use serde_json::json;

/// Adds both registry-backed controls after render distance using the installed pack templates.
pub fn install(catalog: &mut Catalog) {
    let far = SettingId::FarTerrain.definition();
    super::settings_controls::install_video_dropdown(
        catalog,
        far.id,
        "video_section",
        "render_distance_slider",
    );
    let full = SettingId::FullDetailDistance.definition();
    let name = full.name;
    catalog.overlay_text(
        "ui/cinnabar_full_detail_distance.json",
        &json!({
            "namespace": "general_section",
            "video_section": {"modifications": [{
                "array_name": "controls", "operation": "insert_after", "control_name": far.name,
                "value": [{format!("{name}@settings_common.option_slider"): {
                    "$option_label": format!("#{name}_slider_label"),
                    "$slider_name": name,
                    "$slider_value_binding_name": format!("#{name}"),
                    "$slider_tts_text_value": format!("#{name}_text_value"),
                    "$option_enabled_binding_name": format!("#{name}_enabled"),
                    "$slider_steps_binding_name": format!("#{name}_steps"),
                    "$label_bindings": [{"binding_name": "$option_label"}]
                }}]
            }]}
        })
        .to_string(),
    );
}
