//! The host screen wrapping a panel's authored surface and its popup widget overlay.

use json_ui::Catalog;
use serde_json::json;
use ui::mod_panel::Panel;

/// Whether two panels share their document and immutable control schema.
pub(crate) fn same_shape(a: &Panel, b: &Panel) -> bool {
    same_template(a, b) && a.surface.same_definition(&b.surface)
}

/// Facts determining the host wrapper and the control kinds surface actions route to. Popup
/// colours are read when a popup opens, so they never change a route.
pub(super) fn same_template(a: &Panel, b: &Panel) -> bool {
    a.toggle_key == b.toggle_key
        && a.reference_size == b.reference_size
        && a.controls.len() == b.controls.len()
        && a.controls
            .iter()
            .zip(&b.controls)
            .all(|(a, b)| a.same_schema(b))
}

/// Loads the panel's surface under the host screen, with the open popup editor when present.
pub(super) fn catalog(
    panel: &Panel,
    viewport: [f64; 2],
    editor: Option<&super::edit::Editor>,
) -> Result<Catalog, String> {
    catalog_prepared(panel, viewport, editor, None)
}

/// Builds the popup editor overlay over a prepared route without reparsing the local document.
pub(super) fn catalog_prepared(
    panel: &Panel,
    viewport: [f64; 2],
    editor: Option<&super::edit::Editor>,
    prepared: Option<&Catalog>,
) -> Result<Catalog, String> {
    let surface = &panel.surface;
    let mut catalog = match prepared {
        Some(catalog) => catalog.clone(),
        None => super::surface::catalog(surface)?,
    };
    let mut document = json!({"namespace":"cinnabar_personal", "panel": {
        "type":"screen", "size":["100%", "100%"], "render_game_behind":true,
        "absorbs_input":true, "should_steal_mouse":false,
        "controls":[{format!("provided@{}", surface.screen): {"size":["100%","100%"]}}]
    }});
    super::edit::append_overlay(&mut document, panel, viewport, editor);
    catalog.overlay_text("ui/host_extension_panel.json", &document.to_string());
    Ok(catalog)
}
