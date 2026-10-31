use super::*;

/// Builds an independently owned catalog whose documents differ between tests.
fn bundle(name: &str) -> Arc<PanelBundle> {
    let mut panel = crate::test_support::sample_panel();
    panel.surface.document = serde_json::json!({
        "namespace": "extension",
        "screen": {"type": "screen", "size": ["100%", "100%"], "controls": [], "$fixture": name}
    })
    .to_string();
    panel.surface.screen = "extension.screen".into();
    Arc::new(PanelBundle {
        panel,
        surfaces: Default::default(),
        control_schemas: Default::default(),
    })
}

/// Reloading identical input reuses expensive preparation while the old owner still lives.
#[test]
fn prepared_catalog_reuses_identical_reload_and_releases_on_unload() {
    let source = bundle("reuse and unload");
    let first = PreparedPanelUi::prepare(Arc::clone(&source)).unwrap();
    let independent = Arc::new(source.as_ref().clone());
    let reloaded = PreparedPanelUi::prepare(independent).unwrap();
    assert!(
        !Arc::ptr_eq(&first, &reloaded),
        "a reload needs a fresh publisher identity"
    );
    assert!(Arc::ptr_eq(&first.catalog, &reloaded.catalog));
    let weak = Arc::downgrade(&first.catalog);
    drop(first);
    drop(reloaded);
    assert!(
        weak.upgrade().is_none(),
        "the cache must not retain an unloaded catalog"
    );
    let fresh = PreparedPanelUi::prepare(source).unwrap();
    assert!(weak.upgrade().is_none());
    assert!(!fresh.catalog.routes.is_empty());
}

/// Source and action-schema changes must produce fresh validated route state.
#[test]
fn prepared_catalog_invalidates_documents_schemas_and_invalid_inputs() {
    let source = bundle("changed input");
    let first = PreparedPanelUi::prepare(Arc::clone(&source)).unwrap();
    let mut document = source.as_ref().clone();
    document.panel.surface.document = document
        .panel
        .surface
        .document
        .replace("changed input", "new document");
    let changed = PreparedPanelUi::prepare(Arc::new(document)).unwrap();
    assert!(!Arc::ptr_eq(&first.catalog, &changed.catalog));
    let mut schema = source.as_ref().clone();
    schema.panel.controls.push(ui::mod_panel::Control::Button {
        id: "new_action".into(),
        label: "New action".into(),
    });
    let changed = PreparedPanelUi::prepare(Arc::new(schema)).unwrap();
    assert!(!Arc::ptr_eq(&first.catalog, &changed.catalog));
    let mut routes = source.as_ref().clone();
    routes
        .surfaces
        .insert("options".into(), routes.panel.surface.clone());
    let routed = PreparedPanelUi::prepare(Arc::new(routes.clone())).unwrap();
    assert!(!Arc::ptr_eq(&first.catalog, &routed.catalog));
    routes.control_schemas.insert(
        "options".into(),
        vec![ui::mod_panel::Control::Button {
            id: "route_action".into(),
            label: "Route action".into(),
        }],
    );
    let schema = PreparedPanelUi::prepare(Arc::new(routes)).unwrap();
    assert!(!Arc::ptr_eq(&routed.catalog, &schema.catalog));
    let mut invalid = source.as_ref().clone();
    invalid.panel.toggle_key.clear();
    assert!(PreparedPanelUi::prepare(Arc::new(invalid)).is_err());
}
