//! The GUI-scale slider steps through viewport-derived percentages and commits signed modifiers.

use super::super::theme::{BODY, TEXT};
use super::{Content, controls};
use launcher::menu::MenuAction;
use ui_engine::engine::screen::PaintError;
use ui_engine::engine::screen::Translate;

fn label(percentage: u16, translate: Translate<'_>) -> String {
    translate("options.percent.format").map_or_else(
        || format!("{percentage}%"),
        |format| {
            format
                .replace("%s", &percentage.to_string())
                .replace("%%", "%")
        },
    )
}

/// Draws the GUI-scale row as a stepped slider, one stop per available modifier. A window
/// with a single modifier shows the slider disabled at that value.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let view = content.view;
    let choices = &view.gui_scale_choices;
    let title = content.word("options.guiScale.optionName.name");
    let description = content.word(if choices.len() > 1 {
        "options.guiScale.optionName.description"
    } else {
        "options.guiScale.disabled"
    });
    let selected = choices
        .iter()
        .position(|choice| choice.offset == view.gui_scale_offset)
        .unwrap_or(0);
    let shown = choices
        .get(selected)
        .map(|choice| label(choice.percentage, content.translate))
        .unwrap_or_default();
    let actions: Vec<_> = choices
        .iter()
        .map(|choice| MenuAction::SettingsScale(choice.offset))
        .collect();
    let [left, right] = content.inset();
    let amount = content.canvas.measure(&shown, BODY)?;
    let width = (right - left - amount - content.canvas.r(1.6)).max(1.0);
    let extra = content.canvas.r(4.0);
    let b = content.row(&title, &description, width, None, extra)?;
    content.texts(b, &title, &description, width, extra)?;
    content.canvas.text(
        &shown,
        [right - amount, b[1] + content.canvas.r(1.2)],
        amount + 1.0,
        BODY,
        TEXT,
        false,
    )?;
    controls::slider(
        content.canvas,
        view,
        [
            left,
            b[3] - content.canvas.r(4.4),
            right,
            b[3] - content.canvas.r(1.2),
        ],
        &actions,
        selected,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_scale_percentage_uses_the_active_translation_format() {
        let translated: Translate<'_> =
            &|key| (key == "options.percent.format").then(|| std::sync::Arc::from("%s %%"));
        assert_eq!(label(67, translated), "67 %");
        assert_eq!(label(50, &|_| None), "50%");
    }
}
