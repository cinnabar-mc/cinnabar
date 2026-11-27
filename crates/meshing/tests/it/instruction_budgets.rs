//! Instruction budgets for sub-chunk meshing; see `test_instructions`.

use assets::NetworkIdMode;
use meshing::{BlockClassifier, mesh_sub_chunk_in_neighbourhood};
use world::MeshNeighbourhood;

#[path = "../support/liquid_fixtures.rs"]
mod fixtures;

/// Meshes every shared liquid and dry-terrain scene once, after a warm-up pass.
#[test]
fn instruction_budget_mesh_liquid_and_terrain_scenes() {
    let assets = fixtures::runtime_assets();
    let classifier = BlockClassifier::new(fixtures::AIR);
    let scenes: Vec<_> = fixtures::SCENES.map(fixtures::scene).into();
    let mesh_all = || {
        scenes
            .iter()
            .map(|center| {
                let neighbourhood = MeshNeighbourhood::new(center);
                let mesh = mesh_sub_chunk_in_neighbourhood(
                    &classifier,
                    &assets,
                    NetworkIdMode::Sequential,
                    &neighbourhood,
                );
                mesh.liquid_quads().len()
            })
            .sum::<usize>()
    };
    let warm = mesh_all();
    let counted = test_instructions::count(mesh_all);
    assert_eq!(counted, warm);
    assert!(counted > 0, "the scenes hold liquid surfaces");
}
