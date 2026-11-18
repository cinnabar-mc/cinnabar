//! Video controls for the vanilla renderer's client-side dynamic lights.

use json_ui::Catalog;
use launcher::menu::settings_options::SettingId;
use serde_json::json;

/// Adds the light toggle and an explanation to Video settings.
pub fn install(catalog: &mut Catalog) {
    let enabled = SettingId::DynamicLights.definition();
    let description = launcher::menu::settings_options::DYNAMIC_LIGHTS_DESCRIPTION;
    let controls = json!([
        {format!("{}@settings_common.option_toggle", enabled.name): {
            "$option_label": enabled.label,
            "$option_binding_name": format!("#{}", enabled.name),
            "$option_enabled_binding_name": format!("#{}_enabled", enabled.name),
            "$toggle_name": enabled.name
        }},
        {"dynamic_lights_description": {
            "type": "label",
            "text": description,
            "localize": false,
            "size": ["100%", "default"],
            "font_size": "small",
            "wrap": true
        }}
    ]);
    catalog.overlay_text(
        "ui/cinnabar_dynamic_lights.json",
        &json!({
            "namespace": "general_section",
            "video_section": {"modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": "render_clouds_toggle",
                "value": controls
            }]}
        })
        .to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(serde_json::Value::as_str)
            == Some(text)
            || control
                .children
                .iter()
                .any(|child| contains_text(child, text))
    }

    #[test]
    fn video_settings_explain_and_expose_dynamic_light_controls() {
        let Some(carrier) = ui_engine::test_support::carrier() else {
            eprintln!(
                "skipping video_settings_explain_and_expose_dynamic_light_controls: missing UI carrier; make assets"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog = Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes)))
            .expect("UI catalog");
        install(&mut catalog);
        let section = json_ui::resolve(
            &catalog,
            "general_section.video_section",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("Video settings");
        assert!(contains_text(
            &section,
            SettingId::DynamicLights.definition().label
        ));
        assert!(contains_text(
            &section,
            launcher::menu::settings_options::DYNAMIC_LIGHTS_DESCRIPTION
        ));
    }
}
