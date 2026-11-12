//! The status effects control a server HUD layer inherits.
//!
//! The built-in Java pack hangs its effect rows from the screen's top-right corner. A server
//! pack that authors HUD controls placed its widgets around vanilla's right-edge column, which
//! starts below the top band, so the Java rows would cover whatever it draws across the top.
//! Such a layer patches the carrier's control, as it would on Bedrock.

use json_ui::{Catalog, RawControl};

const NAMESPACE: &str = "hud";
/// The panel holding the effects control, and the control's instance name.
const PANEL: &str = "root_panel";
const CONTROL: &str = "mob_effects_renderer";

/// Whether the layer authors any control of the HUD namespace.
pub(super) fn authored(catalog: &Catalog, files: &[(String, Vec<u8>)]) -> bool {
    catalog
        .overlay_controls(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        )
        .iter()
        .any(|control| control.namespace == NAMESPACE)
}

/// Replaces the built-in pack's effects control with the carrier's.
pub(super) fn restore(catalog: &mut Catalog, base: &Catalog) {
    let Some(vanilla) = base.lookup(NAMESPACE, PANEL).and_then(control) else {
        return;
    };
    let vanilla = vanilla.clone();
    if let Some(slot) = catalog.lookup_mut(NAMESPACE, PANEL).and_then(|panel| {
        panel
            .children
            .iter_mut()
            .find(|child| child.name == CONTROL)
    }) {
        *slot = vanilla;
    }
}

fn control(panel: &RawControl) -> Option<&RawControl> {
    panel.children.iter().find(|child| child.name == CONTROL)
}
