//! Displayed option labels and help text shared by category rows and search.

use super::Content;
use launcher::menu::settings_options::SettingId;

impl Content<'_, '_> {
    /// Resolves the same translated labels and client help text used by editable rows.
    pub(super) fn option_text(&self, id: SettingId) -> (String, String) {
        let option = id.definition();
        let label_key = if id == SettingId::GuiScale {
            "options.guiScale.optionName.name"
        } else {
            option.oreui_label.unwrap_or(option.label)
        };
        let title = self.word(label_key);
        let base_key = label_key.strip_suffix(".name").unwrap_or(label_key);
        let description_key = match id {
            SettingId::KeyboardMouseInvertYAxis => {
                "options.invertYAxis.Mouse.description".to_owned()
            }
            SettingId::ControllerInvertYAxis => "options.invertYAxis.description".to_owned(),
            SettingId::TouchControlMode => {
                launcher::menu::settings_options::touch_control_mode::description_key(
                    self.view.settings_options.touch_control_mode(),
                )
                .to_owned()
            }
            _ => format!("{base_key}.description"),
        };
        let description = super::localization::locale_text(
            &description_key,
            self.view.settings_options.language(),
        )
        .map(str::to_owned)
        .or_else(|| (self.translate)(&description_key).map(|text| text.to_string()))
        .unwrap_or_else(|| {
            let fallback = super::localization::fallback(&description_key);
            if fallback != description_key {
                fallback.to_owned()
            } else {
                String::new()
            }
        });
        (title, description)
    }
}
