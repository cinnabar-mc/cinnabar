use json_ui::Catalog;

const ACTIONBAR: &str = json_ui::vanilla::Control::HudActionbarText.as_str();

/// Whether a server layer authors the actionbar definition or a control below it.
pub(super) fn authored(catalog: &Catalog, files: &[(String, Vec<u8>)]) -> bool {
    catalog
        .overlay_controls(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        )
        .iter()
        .any(|control| {
            control.namespace == "hud" && control.name.split('/').next() == Some(ACTIONBAR)
        })
}

/// Server actionbar patches inherit carrier placement, not the built-in HUD's anchors.
pub(super) fn restore(catalog: &mut Catalog, base: &Catalog) {
    if let Some(actionbar) = base.lookup("hud", ACTIONBAR) {
        catalog.insert(actionbar.clone());
    }
}
