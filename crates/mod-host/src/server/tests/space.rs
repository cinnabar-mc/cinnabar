use super::*;

#[test]
fn replayed_drawing_epoch_replaces_space_resources() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/experiences/space-client");
    let manifest: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.join("experience.toml")).unwrap()).unwrap();
    let permissions: BTreeSet<Permission> = manifest["client"]["permissions"]
        .clone()
        .try_into()
        .unwrap();
    let paths: BTreeSet<String> = manifest["client"]["assets"].clone().try_into().unwrap();
    let assets = paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(root.join(&path)).unwrap();
            (path, bytes)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut capabilities = screen_capabilities(&[]);
    capabilities.scope.permissions = permissions;
    capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    capabilities.assets = assets.keys().cloned().collect();
    let mut host = BundleHost::launch(
        &guests::component("experience-space-client"),
        owner(),
        capabilities,
        1,
        assets,
        drawing_context(1),
    )
    .unwrap();
    for epoch in [1, 1, 2, 2] {
        host.dispatch(&Event::Epoch, epoch, &drawing_context(epoch))
            .unwrap()
            .into_transaction();
        let output = host.store.data().presentation.presentation_output();
        assert_eq!(output.drawing.instances.len(), 2);
        assert_eq!(output.drawing.geometry.len(), 2);
        assert_eq!(output.drawing.materials.len(), 1);
    }
}
