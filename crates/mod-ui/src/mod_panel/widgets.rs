//! JSON-UI builders and colours of the host-drawn popup widgets: the choice dropdown and the
//! slider and text value editor with its hint.

use serde_json::{Value, json};
use ui::mod_panel::{FONT_NAME, Panel, WidgetColors};

/// Body labels and their native editor use the same font scale.
pub(super) const LABEL_SCALE: f64 = 0.85;

/// Popup widget colours, from the panel's `widget_colors` over the built-in dark palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// The popup's own fill.
    pub card: [f64; 4],
    pub border: [f64; 4],
    pub text: [f64; 4],
    /// Fields and the highlighted row.
    pub raised: [f64; 4],
    pub accent: [f64; 4],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            card: [0.055, 0.058, 0.065, 0.96],
            border: [0.2, 0.21, 0.23, 0.94],
            text: [0.94, 0.95, 0.97, 1.0],
            raised: [0.15, 0.16, 0.18, 1.0],
            accent: [0.96, 0.46, 0.40, 1.0],
        }
    }
}

impl Palette {
    /// The built-in palette with each colour the panel authors replaced.
    pub fn for_panel(panel: &Panel) -> Self {
        let mut palette = Self::default();
        let Some(colors) = panel.widget_colors else {
            return palette;
        };
        let WidgetColors {
            background,
            surface,
            text,
            accent,
            border,
        } = colors;
        for (slot, color) in [
            (&mut palette.card, background),
            (&mut palette.raised, surface),
            (&mut palette.text, text),
            (&mut palette.accent, accent),
            (&mut palette.border, border),
        ] {
            if let Some(color) = color {
                *slot = color.map(f64::from);
            }
        }
        palette
    }
}

pub(super) fn named(name: &str, definition: Value) -> Value {
    json!({name:definition})
}

pub(super) fn rounded(size: [f64; 2], offset: [f64; 2], radius: f64, color: [f64; 4]) -> Value {
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","radius":radius,"color":color})
}

pub(super) fn panel(size: [f64; 2], offset: [f64; 2], controls: Vec<Value>) -> Value {
    json!({"type":"panel","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","controls":controls})
}

pub(super) fn button(
    action: &str,
    size: [f64; 2],
    offset: [f64; 2],
    controls: Vec<Value>,
) -> Value {
    let mut node = panel(size, offset, controls);
    node["type"] = json!("button");
    node["button_mappings"] = json!([{"from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"}]);
    node
}

pub(super) fn label(
    text: &str,
    size: [f64; 2],
    offset: [f64; 2],
    color: [f64; 4],
    binding: bool,
) -> Value {
    let mut label = json!({"type":"label","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","text":text,"text_alignment":"left",
        "color":color,"shadow":false,"clip_children":true,"hide_hyphen":true,"localize":false,"font_type":FONT_NAME,"font_scale_factor":LABEL_SCALE});
    if binding {
        label["bindings"] = json!([{"binding_name":text}]);
    }
    label
}

/// A bordered rounded popup background of `size`.
pub(super) fn chrome(size: [f64; 2], palette: Palette, radius: f64) -> Vec<Value> {
    vec![
        named("border", rounded(size, [0.0; 2], radius, palette.border)),
        named(
            "surface",
            rounded(
                [size[0] - 1.0, size[1] - 1.0],
                [0.5; 2],
                radius - 0.5,
                palette.card,
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_colors_replace_only_the_authored_palette_slots() {
        let mut panel: Panel = serde_json::from_value(json!({"toggle_key":"F10",
            "surface":{"screen":"p.root","document":json!({"namespace":"p","root":{"type":"panel"}}).to_string()}}))
        .unwrap();
        assert_eq!(Palette::for_panel(&panel), Palette::default());
        panel.widget_colors = Some(WidgetColors {
            accent: Some([0., 1., 0., 1.]),
            background: Some([0., 0., 0., 0.5]),
            ..Default::default()
        });
        let palette = Palette::for_panel(&panel);
        assert_eq!(palette.accent, [0., 1., 0., 1.]);
        assert_eq!(palette.card, [0., 0., 0., 0.5]);
        assert_eq!(palette.text, Palette::default().text);
    }
}
