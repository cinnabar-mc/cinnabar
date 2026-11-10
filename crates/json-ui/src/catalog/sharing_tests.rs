//! Layered catalogs stay resident together, so copies share unchanged definitions.

use super::Catalog;

/// A document of `count` controls, each with a few properties and nested children.
fn document(count: usize) -> String {
    let controls = (0..count)
        .map(|index| {
            format!(
                r#""control_{index}@common.base":{{"size":[{index},10],"offset":[1,2],"text":"label {index}","controls":[{{"child@common.child":{{"alpha":0.5,"layer":{index}}}}}]}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"namespace":"screen",{controls}}}"#)
}

/// Bytes `work` requested on this thread.
fn allocated<T>(work: impl FnOnce() -> T) -> (T, usize) {
    let (value, allocations) = test_alloc::measure(work);
    (value, allocations.bytes)
}

#[test]
fn cloning_a_catalog_shares_its_definitions() {
    let mut catalog = Catalog::default();
    let text = document(500);
    let ((), parsed) = allocated(|| catalog.overlay_text("ui/screen.json", &text));
    let (copy, cloned) = allocated(|| catalog.clone());
    assert!(
        cloned < parsed / 4,
        "cloning allocated {cloned} bytes after parsing allocated {parsed}"
    );
    assert_eq!(
        copy.lookup("screen", "control_7"),
        catalog.lookup("screen", "control_7")
    );
}

#[test]
fn changing_a_shared_definition_leaves_the_other_copy_unchanged() {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/screen.json", &document(3));
    let original = catalog.clone();
    catalog.overlay_text(
        "ui/pack.json",
        r#"{"namespace":"screen","control_1":{"text":"changed"},"control_2/child":{"alpha":1}}"#,
    );
    catalog
        .lookup_mut("screen", "control_0")
        .unwrap()
        .props
        .clear();
    assert_eq!(
        catalog.lookup("screen", "control_1").unwrap().props["text"],
        "changed"
    );
    assert_eq!(
        original.lookup("screen", "control_1").unwrap().props["text"],
        "label 1"
    );
    assert_eq!(
        original.lookup("screen", "control_2").unwrap().children[0].props["alpha"],
        0.5
    );
    assert_eq!(
        catalog.lookup("screen", "control_2").unwrap().children[0].props["alpha"],
        1
    );
    assert!(
        !original
            .lookup("screen", "control_0")
            .unwrap()
            .props
            .is_empty()
    );
}
