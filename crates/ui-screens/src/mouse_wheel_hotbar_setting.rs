//! Keyboard/mouse toggle for wheel-driven hotbar cycling.

use json_ui::Catalog;
use launcher::menu::settings_options::SettingId;
use serde_json::json;

/// Adds the wheel toggle directly after Always Sprint in Keyboard & Mouse.
pub fn install(catalog: &mut Catalog) {
    let name = SettingId::MouseWheelHotbarScrolling.definition().name;
    catalog.overlay_text(
        "ui/cinnabar_mouse_wheel_hotbar_scrolling.json",
        &json!({
            "namespace": "controls_section",
            "keyboard_and_mouse_section": {
                "modifications": [{
                    "array_name": "controls",
                    "operation": "insert_after",
                    "control_name": "always_sprint",
                    "value": [{
                        format!("{name}@settings_common.option_toggle"): {
                            "$option_label": ui::MOUSE_WHEEL_HOTBAR_SCROLLING_LABEL,
                            "$option_binding_name": format!("#{name}"),
                            "$option_enabled_binding_name": format!("#{name}_enabled"),
                            "$toggle_name": name,
                            "$focus_override_right": "FOCUS_OVERRIDE_STOP"
                        }
                    }]
                }]
            }
        })
        .to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The toggle follows the Always Sprint row it is anchored to.
    #[test]
    fn keyboard_mouse_options_show_wheel_toggle_after_always_sprint() {
        let Some(carrier) = ui_engine::test_support::carrier() else {
            eprintln!(
                "skipping keyboard_mouse_options_show_wheel_toggle_after_always_sprint: missing UI carrier; make assets"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        super::super::always_sprint_setting::install(&mut catalog);
        install(&mut catalog);
        let section = json_ui::resolve(
            &catalog,
            "controls_section.keyboard_and_mouse_section",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("Keyboard & Mouse options");
        let position = |name: &str| section.children.iter().position(|child| child.name == name);
        let wheel = position(SettingId::MouseWheelHotbarScrolling.definition().name)
            .expect("wheel toggle in Keyboard & Mouse options");
        let sprint = position("always_sprint").expect("Always Sprint in Keyboard & Mouse options");
        assert_eq!(wheel, sprint + 1);
    }
}
