use super::*;

#[path = "../../tests/support/liquid_fixtures.rs"]
mod fixtures;

/// Checks both fill orders, repeated hits, all corner heights and flow against uncached resolution.
fn assert_matches_direct(assets: &RuntimeAssets, center: &world::SubChunk) {
    let classifier = BlockClassifier::new(fixtures::AIR);
    let mut neighbourhood = MeshNeighbourhood::new(center);
    // Known and unknown sources share every halo face; wider samples use the direct fallback.
    for x in -1_i8..=1 {
        for y in -1_i8..=1 {
            for z in -1_i8..=1 {
                if (x + y + z) % 2 != 0 {
                    assert!(neighbourhood.insert([x, y, z], center));
                }
            }
        }
    }
    let mut cached = Sampler::new(
        classifier,
        assets,
        NetworkIdMode::Sequential,
        &neighbourhood,
    );
    let mut direct = DirectSampler {
        classifier,
        assets,
        mode: NetworkIdMode::Sequential,
    };
    let coordinates: Vec<_> = (-2..=17)
        .flat_map(|x| (-2..=17).flat_map(move |y| (-2..=17).map(move |z| [x, y, z])))
        .chain([[-16, 0, 0], [31, 0, 0], [-17, 0, 0], [32, 0, 0]])
        .collect();
    for pass in 0..3 {
        for &coordinate in &coordinates {
            if pass != 0 || coordinate[0] % 2 != 0 {
                assert_eq!(
                    cached.occlusion_part(&neighbourhood, coordinate),
                    direct.occlusion_part(&neighbourhood, coordinate),
                    "occlusion at {coordinate:?}"
                );
            }
            assert_eq!(
                cached.liquid_part(&neighbourhood, coordinate),
                direct.liquid_part(&neighbourhood, coordinate),
                "liquid at {coordinate:?}"
            );
            if pass == 0 && coordinate[0] % 2 == 0 {
                assert_eq!(
                    cached.occlusion_part(&neighbourhood, coordinate),
                    direct.occlusion_part(&neighbourhood, coordinate),
                    "occlusion at {coordinate:?}"
                );
            }
        }
    }
    for x in 0..16 {
        for y in 0..16 {
            for z in 0..16 {
                let coordinate = [x, y, z];
                let Some(cell) = direct.liquid(&neighbourhood, coordinate) else {
                    continue;
                };
                assert_eq!(
                    corner_heights(&mut cached, &neighbourhood, coordinate, cell.identity),
                    corner_heights(&mut direct, &neighbourhood, coordinate, cell.identity),
                    "corners at {coordinate:?}"
                );
                assert_eq!(
                    flow_gradient(&mut cached, &neighbourhood, coordinate, cell),
                    flow_gradient(&mut direct, &neighbourhood, coordinate, cell),
                    "flow at {coordinate:?}"
                );
            }
        }
    }
}

#[test]
fn liquid_sampler_matches_direct_for_dense_flowing_sparse_and_overflow_scenes() {
    let assets = fixtures::runtime_assets();
    for name in fixtures::SCENES {
        assert_matches_direct(&assets, &fixtures::scene(name));
    }
}

#[test]
fn liquid_sampler_preserves_layered_unsupported_and_unknown_cells() {
    let assets = fixtures::runtime_assets();
    let primary = fixtures::values(|[x, y, z]| match (x + y + z) % 6 {
        0 => fixtures::SOLID,
        1 => fixtures::GLASS,
        2 => fixtures::INVALID_LEVEL,
        3 => fixtures::UNSUPPORTED,
        4 => u32::MAX,
        _ => fixtures::AIR,
    });
    let liquid = fixtures::values(|[x, y, z]| {
        if (x + y + z) % 3 == 0 {
            fixtures::AIR
        } else {
            1 + u32::from((x + y + z) % 32)
        }
    });
    assert_matches_direct(&assets, &fixtures::chunk(&[primary, liquid]));
}
