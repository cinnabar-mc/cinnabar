use super::*;

const CROSSINGS: usize = 32;

/// Measures successive camera boundary crossings in a settled resident graph.
pub(super) fn camera_benches(c: &mut Criterion) {
    let assets = Arc::new(terrain_assets());
    let mut group = c.benchmark_group("pipeline/cave_camera_boundaries");
    if !cfg!(feature = "runtime-profile") {
        group.sample_size(20);
    }
    for radius in [10, 16] {
        let flight = Flight::new(&assets, radius);
        for (name, path) in [("straight", straight()), ("circle", circle())] {
            // The synchronous search measures the full traversal cost a worker absorbs.
            let mut rebuild = CaveVisibilityRebuild::synchronous();
            let mut scratch = CaveVisibilityScratch::default();
            let mut visible = CaveVisibleSet::default();
            let mut replacement = CaveVisibleSet::default();
            cross(
                &flight,
                path[CROSSINGS - 1],
                &mut rebuild,
                &mut scratch,
                &mut visible,
                &mut replacement,
            );
            let mut exits = 0;
            let mut proofs = 0;
            let mut rebuilds = 0;
            for &camera in &path {
                cross(
                    &flight,
                    camera,
                    &mut rebuild,
                    &mut scratch,
                    &mut visible,
                    &mut replacement,
                );
                let work = scratch.work();
                exits += work.explored_exits;
                proofs += work.proof_exits;
                rebuilds += usize::from(work.rebuilt);
            }
            eprintln!(
                "CAVE_CROSSINGS radius={radius} path={name} crossings={CROSSINGS} exits={exits} proof_exits={proofs} rebuilds={rebuilds}"
            );
            group.bench_function(BenchmarkId::new(format!("radius_{radius}"), name), |b| {
                b.iter(|| {
                    for &camera in &path {
                        cross(
                            &flight,
                            camera,
                            &mut rebuild,
                            &mut scratch,
                            &mut visible,
                            &mut replacement,
                        );
                    }
                    black_box(&visible);
                });
            });
        }
    }
    group.finish();
}

/// Applies the same scratch/output buffer contract as the renderer.
fn cross(
    flight: &Flight,
    camera: SubChunkKey,
    rebuild: &mut CaveVisibilityRebuild,
    scratch: &mut CaveVisibilityScratch,
    visible: &mut CaveVisibleSet,
    replacement: &mut CaveVisibleSet,
) {
    if flight
        .stream
        .update_cave_visible_sub_chunks(camera, rebuild, scratch, visible, replacement)
        == CaveVisibilityUpdate::Rebuilt
    {
        std::mem::swap(visible, replacement);
    }
}

/// Flies back and forth across sixteen adjacent boundaries without leaving the loaded view.
fn straight() -> [SubChunkKey; CROSSINGS] {
    std::array::from_fn(|i| {
        let x = if i < 16 { i as i32 - 8 } else { 24 - i as i32 };
        SubChunkKey::new(0, x, (EYE_Y as i32).div_euclid(16), 0)
    })
}

/// Rounds a square circuit with one adjacent boundary crossing per sample.
fn circle() -> [SubChunkKey; CROSSINGS] {
    std::array::from_fn(|i| {
        let n = (i % 8) as i32;
        let (x, z) = match i / 8 {
            0 => (n - 4, -4),
            1 => (4, n - 4),
            2 => (4 - n, 4),
            _ => (-4, 4 - n),
        };
        SubChunkKey::new(0, x, (EYE_Y as i32).div_euclid(16), z)
    })
}
