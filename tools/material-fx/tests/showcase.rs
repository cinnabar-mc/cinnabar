//! The showcase builds into an artifact the Experience runtime loads and a client part bundle
//! the client verifies, both from the real guests.

use material_fx::artifact::{self, Guests, ID};
use std::path::Path;

/// The client part's materials, compiled natively so the test reads the same table the guest
/// declares from.
#[path = "../../../examples/experiences/material-fx/client/src/effects.rs"]
#[allow(dead_code)]
mod effects;

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
    let mut looks: Vec<String> = artifact::BLOCK_LOOKS
        .iter()
        .map(|(name, ..)| format!("{ID}:{name}"))
        .collect();
    looks.sort_unstable();
    assert_eq!(
        blocks, looks,
        "every registered block has a Bedrock look and no other does"
    );
    every_bedrock_look_resolves(&loaded.blocks);
    glass_blocks_blend_like_their_materials(&loaded.blocks);
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

/// The Bedrock look paths of `block`: its cube textures, or its visual's materials.
fn look_paths(block: &experience_runtime::protocol::BlockDef) -> Vec<&str> {
    match &block.visual {
        Some(visual) => visual.materials.iter().map(|m| m.path.as_str()).collect(),
        None => block.textures.iter().map(|t| t.path.as_str()).collect(),
    }
}

/// Every block shows a Bedrock look that exists in the artifact and decodes as a 16×16 image,
/// so a block whose material is revoked or still compiling never draws the missing texture.
fn every_bedrock_look_resolves(blocks: &[experience_runtime::protocol::BlockDef]) {
    for block in blocks {
        let paths = look_paths(block);
        assert!(!paths.is_empty(), "{} has no Bedrock look", block.id);
        for path in paths {
            let image = image::open(path)
                .unwrap_or_else(|error| panic!("{}'s look {path}: {error}", block.id));
            assert_eq!(
                (image.width(), image.height()),
                (16, 16),
                "{}'s look {path}",
                block.id
            );
        }
    }
}

/// A block whose world material lets the world behind it show has a Bedrock look that blends
/// too, so neither hides its neighbours' faces; every other block is an opaque cube.
fn glass_blocks_blend_like_their_materials(blocks: &[experience_runtime::protocol::BlockDef]) {
    use experience_runtime::protocol::RenderMethod;
    use experience_sdk::client::world_material::{Blend, Target};
    for effect in &effects::EFFECTS {
        let Some(Target::Block(id)) = (effect.target)() else {
            continue;
        };
        let block = blocks.iter().find(|block| block.id == id).unwrap();
        let blends = block.visual.as_ref().is_some_and(|visual| {
            visual
                .materials
                .iter()
                .all(|material| material.render_method == RenderMethod::Blend)
        });
        assert_eq!(blends, effect.blend != Blend::Opaque, "{id}");
    }
}
