//! Development packs reload on Apply only when their folders changed.

use super::*;
use launcher::global_resources::{Action, Snapshot};

/// Writes an original development pack whose only resource is `marker.txt`.
fn write_pack(folder: &std::path::Path, marker: &[u8]) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(
        folder.join("manifest.json"),
        br#"{"format_version":2,"header":{"uuid":"00000000-0000-0000-0000-00000000d001","name":"Working copy","version":[1,0,0]},"modules":[{"type":"resources"}]}"#,
    )
    .unwrap();
    std::fs::write(folder.join("marker.txt"), marker).unwrap();
}

/// Applies the staged selection and counts the stacks sent to the runtime.
fn apply(library: &mut GlobalPackLibrary, snapshot: &mut Snapshot, applied: &mut Applied) -> usize {
    let (outgoing, events) = crossbeam_channel::unbounded();
    act(library, snapshot, Action::Apply, &outgoing, applied).unwrap();
    events
        .try_iter()
        .filter(|event| matches!(event, Event::Apply(..)))
        .count()
}

#[test]
fn apply_reloads_development_packs_only_when_their_folder_changed() {
    let fixture = tempfile::tempdir().unwrap();
    let roots = crate::global_resources::Roots::under(&fixture.path().join("installed"));
    let folder = roots.development.join("working_copy");
    write_pack(&folder, b"first");
    let mut library = GlobalPackLibrary::open(&roots.packs, engine_version()).unwrap();
    library.set_development_root(&roots.development);
    let id = library.installed().next().unwrap().id;
    library.activate(id).unwrap();
    let mut snapshot = Snapshot::default();
    let mut applied = Applied::default();

    assert_eq!(apply(&mut library, &mut snapshot, &mut applied), 1);
    let selection = library.active().to_vec();
    snapshot.applied_selection = Some(selection.clone());
    applied.acknowledge(&selection);
    refresh(&library, &mut snapshot);
    assert!(snapshot.development_active);
    assert!(snapshot.can_apply(), "Apply stays enabled for edits");

    assert_eq!(apply(&mut library, &mut snapshot, &mut applied), 0);
    assert_eq!(snapshot.message, "Resource packs are already up to date.");

    write_pack(&folder, b"second");
    assert_eq!(apply(&mut library, &mut snapshot, &mut applied), 1);
}

#[test]
fn development_packs_are_listed_beside_imported_packs() {
    let fixture = tempfile::tempdir().unwrap();
    let roots = crate::global_resources::Roots::under(&fixture.path().join("installed"));
    write_pack(&roots.development.join("working_copy"), b"listed");
    let mut library = GlobalPackLibrary::open(&roots.packs, engine_version()).unwrap();
    library.set_development_root(&roots.development);
    let mut snapshot = Snapshot::default();
    refresh(&library, &mut snapshot);
    assert_eq!(snapshot.available.len(), 1);
    assert_eq!(snapshot.available[0].name, "Working copy");
    assert!(!snapshot.development_active);
}
