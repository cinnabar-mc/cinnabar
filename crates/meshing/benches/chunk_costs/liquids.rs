use super::*;

#[path = "../../tests/support/liquid_fixtures.rs"]
mod fixtures;

/// Measures complete liquid meshing, including memo setup and the empty-section fast path.
pub(super) fn meshing_liquids(c: &mut Criterion) {
    let assets = fixtures::runtime_assets();
    let classifier = BlockClassifier::new(fixtures::AIR);
    let mut group = c.benchmark_group("meshing/liquids");
    for name in fixtures::SCENES {
        let center = fixtures::scene(name);
        let neighbourhood = MeshNeighbourhood::new(&center);
        let mesh = mesh_sub_chunk_in_neighbourhood(&classifier, &assets, MODE, &neighbourhood);
        let fingerprint = mesh
            .liquid_quads()
            .iter()
            .flat_map(|quad| quad.words())
            .chain(
                mesh.liquid_lighting()
                    .iter()
                    .flat_map(|lighting| lighting.samples().map(u32::from)),
            )
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, word| {
                (hash ^ u64::from(word)).wrapping_mul(0x100_0000_01b3)
            });
        eprintln!(
            "liquid fixture {name}: {} quads, {} lighting records, fingerprint={fingerprint:016x}",
            mesh.liquid_quads().len(),
            mesh.liquid_lighting().len()
        );
        group.bench_function(name, |b| {
            b.iter(|| {
                black_box(mesh_sub_chunk_in_neighbourhood(
                    black_box(&classifier),
                    black_box(&assets),
                    MODE,
                    black_box(&neighbourhood),
                ))
            })
        });
    }
    group.finish();
}
