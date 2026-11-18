use json_ui::{DataSource, Scalar};
use serde_json::{Value, json};
use ui::mod_hud::{Card, Hud};

const HEADER: f64 = 18.;

#[cfg(test)]
mod tests;

pub(super) fn named(name: &str, node: Value) -> Value {
    json!({name: node})
}
pub(super) fn rounded(size: [f64; 2], offset: [f64; 2], radius: f64, color: [f32; 4]) -> Value {
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","radius":radius,"color":color})
}
/// A left-aligned single-line label bound to the global `key`.
fn label(key: &str, size: [f64; 2], offset: [f64; 2], scale: f64) -> Value {
    json!({"type":"label","size":size,"offset":offset,"anchor_from":"top_left","anchor_to":"top_left",
        "text":key,"bindings":[{"binding_name":key}],"localize":false,"shadow":true,"hide_hyphen":true,
        "text_alignment":"left","font_scale_factor":0.8*scale,"color":[1,1,1,1],"clip_children":true})
}
/// The card's preview control, drawn under the layout editor's chrome.
pub(in super::super) fn card(card: &Card, card_index: usize) -> Value {
    let mut node = card_content(card, card_index);
    if card.font != ui::mod_hud::CardFont::Native {
        apply_font(&mut node, card.font.name());
    }
    node
}

/// Applies card typography to retained labels.
fn apply_font(node: &mut Value, font: &str) {
    match node {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("label") {
                object.insert("font_type".into(), json!(font));
            }
            for value in object.values_mut() {
                apply_font(value, font);
            }
        }
        Value::Array(values) => {
            for value in values {
                apply_font(value, font);
            }
        }
        _ => {}
    }
}

/// Builds the card's background, optional title and divider before applying its typography.
fn card_content(card: &Card, card_index: usize) -> Value {
    let k = f64::from(card.scale);
    let width = f64::from(card.width);
    let size = dimensions(card);
    let mut controls = vec![
        named(
            "surface",
            rounded(
                size,
                [0.; 2],
                f64::from(card.background_radius) * k,
                [0.045, 0.05, 0.065, card.background_opacity],
            ),
        ),
        named(
            "title",
            label(
                &format!("#card_{card_index}_title"),
                [(width - 12.) * k, 12. * k],
                [6. * k, 5. * k],
                k,
            ),
        ),
        named(
            "divider",
            rounded(
                [(width - 12.) * k, 0.5 * k],
                [6. * k, 15. * k],
                0.,
                [1., 1., 1., 0.16],
            ),
        ),
    ];
    controls[0]["surface"]["bindings"] = json!([{ "binding_name":format!("#card_{card_index}_background"),"binding_name_override":"#color" }]);
    if card.title.is_empty() {
        controls.truncate(1);
    }
    panel_for_card(card_index, size, controls)
}

/// Places one card at its live normalized or anchored position.
fn panel_for_card(card_index: usize, size: [f64; 2], controls: Vec<Value>) -> Value {
    json!({"type":"panel","size":size,"offset":[0,0],
        "anchor_from":"top_left","anchor_to":"top_left","controls":controls,
        "bindings":[{"binding_name":format!("#card_{card_index}_offset"),"binding_name_override":"#offset"}]})
}

/// Binds every card's title, placement and background without rebuilding the catalog.
pub(in super::super) fn data(hud: &Hud, viewport: [f64; 2]) -> DataSource {
    let mut data = DataSource::new();
    for (n, card) in hud.cards.iter().enumerate() {
        data.set_global(format!("#card_{n}_title"), Scalar::Text(card.title.clone()));
        data.set_global(
            format!("#card_{n}_offset"),
            Scalar::Json(json!(origin(card, viewport))),
        );
        data.set_global(
            format!("#card_{n}_background"),
            Scalar::Json(json!([0.045, 0.05, 0.065, card.background_opacity])),
        );
    }
    data
}

/// Card geometry shared by preview painting, hit testing and snapping: the host-measured
/// preview size, or the card width over its optional title band.
pub(in super::super) fn dimensions(card: &Card) -> [f64; 2] {
    if let Some(size) = card.editor_size {
        return size.map(|side| f64::from(side) * f64::from(card.scale));
    }
    let header = if card.title.is_empty() { 0. } else { HEADER };
    [
        f64::from(card.width) * f64::from(card.scale),
        (header + 4.) * f64::from(card.scale),
    ]
}
/// Resolves saved normalized travel or the authored anchored offset.
pub(in super::super) fn origin(card: &Card, viewport: [f64; 2]) -> [f64; 2] {
    let size = dimensions(card);
    let available = std::array::from_fn::<_, 2, _>(|axis| (viewport[axis] - size[axis]).max(0.));
    if let Some(position) = card.position {
        return std::array::from_fn(|axis| f64::from(position[axis]) * available[axis]);
    }
    if card.editor_size.is_some() {
        card.anchor.origin_unclamped(card.offset, size, viewport)
    } else {
        card.anchor.origin(card.offset, size, viewport)
    }
}
