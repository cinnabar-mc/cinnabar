//! The showcase builds into an artifact the Experience runtime loads and a client part bundle
//! the client verifies, both from the real guests.

use material_fx::artifact::{self, Guests, ID};
use std::path::Path;

#[test]
fn the_showcase_builds_a_loadable_artifact_and_a_verified_world_material_bundle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let guests = Guests::build(&root).expect("the guests build");
    let out = tempfile::tempdir().unwrap();
    let built = artifact::build(&root, out.path(), &guests).unwrap();
    let (engine, _ticker) = experience_runtime::load::engine().unwrap();
    let loaded = experience_runtime::load::load(&engine, &built.experiences.join(ID)).unwrap();
    let mut blocks: Vec<_> = loaded
        .blocks
        .iter()
        .map(|block| block.id.as_str())
        .collect();
    blocks.sort_unstable();
    assert_eq!(blocks, ["materialfx:energy", "materialfx:portal"]);
    let bytes = std::fs::read(&built.cxb).unwrap();
    let manifest = server_experience::bundle::offline::verify(&bytes)
        .unwrap()
        .manifest;
    assert_eq!(manifest.id, ID);
    assert_eq!(manifest.namespace.as_deref(), Some(ID));
    assert!(
        manifest
            .permissions
            .contains(&server_experience::manifest::Permission::RenderWorldMaterial)
    );
}
