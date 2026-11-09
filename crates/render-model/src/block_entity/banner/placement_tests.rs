use super::*;

/// Original blank banner artwork at stock or replacement-texture resolution.
fn artwork(side: u32) -> assets::RuntimeBlockEntityAssets {
    let placements: Vec<_> = ["base", "stripe_bottom"]
        .into_iter()
        .enumerate()
        .map(|(index, pattern)| assets::BlockEntityPlacement {
            name: format!("{BANNER_PATTERN_TEXTURE_PREFIX}{pattern}").into(),
            x: index as u32 * side,
            y: 0,
            width: side,
            height: side,
        })
        .collect();
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        side * 2,
        side,
        &vec![255; (side * side * 8) as usize],
        &placements,
    )
    .unwrap();
    assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap()
}

/// An original blank atlas at either stock or replacement-texture resolution.
fn atlas(side: u32) -> BlockEntityAtlas {
    BlockEntityAtlas::from_assets(&artwork(side))
}

/// Compares emitted cloth corners with the mounted, animated attachment contract.
#[test]
fn cloth_attaches_to_crossbar_for_every_mount_and_wave_phase() {
    let mut mounts: Vec<_> = (0..16)
        .map(|rotation| BannerMount::Standing {
            rotation_degrees: rotation as f32 * 22.5,
        })
        .collect();
    mounts
        .extend([Facing::North, Facing::South, Facing::West, Facing::East].map(BannerMount::Wall));
    for side in [64, 128] {
        let atlas = atlas(side);
        for &mount in &mounts {
            let model = BannerModel {
                mount,
                base: [1.0, 0.0, 0.0],
                layers: vec![BannerLayer {
                    pattern: "stripe_bottom",
                    color: [0.0, 1.0, 0.0],
                }]
                .into(),
            };
            let (origin, yaw) = match mount {
                BannerMount::Standing { rotation_degrees } => {
                    (Vec3::new(0.5, 0.48, 0.5), 180.0 - rotation_degrees)
                }
                BannerMount::Wall(facing) => (
                    Vec3::new(0.5, -0.16, 0.5)
                        + Mat4::from_rotation_y(facing.yaw_degrees().to_radians())
                            .transform_vector3(Vec3::new(0.0, -0.3125, 0.4375)),
                    facing.yaw_degrees(),
                ),
            };
            let unmount =
                Mat4::from_rotation_y(-yaw.to_radians()) * Mat4::from_translation(-origin);
            for ticks in [0.0, 25.0, 50.0, 75.0, 100.0] {
                let mut builder = MeshBuilder::new(atlas.size());
                emit(&mut builder, &atlas, [0; 3], &model, SceneClock { ticks });
                let angle = (2.25 - 1.8 * (std::f32::consts::TAU * ticks as f32 / 100.0).cos())
                    .to_radians();
                let uncloth = Mat4::from_rotation_x(-angle)
                    * Mat4::from_translation(Vec3::new(0.0, -1.28, 0.0))
                    * unmount;
                let cloth: Vec<_> = builder
                    .solid
                    .iter()
                    .filter(|vertex| vertex.color[0] > 0.0 && vertex.color[1] == 0.0)
                    .map(|vertex| uncloth.transform_point3(Vec3::from(vertex.position)))
                    .collect();
                assert!(!cloth.is_empty());
                for corner in [
                    [-0.4, 0.0, -0.08],
                    [0.4, 0.0, -0.04],
                    [-0.4, -1.6, -0.08],
                    [0.4, -1.6, -0.04],
                ] {
                    assert!(
                        cloth
                            .iter()
                            .any(|point| point.distance(Vec3::from(corner)) < 1.0e-5),
                        "missing cloth corner {corner:?}: {mount:?}, tick {ticks}, texture {side}"
                    );
                }
                let bar_top = builder
                    .solid
                    .iter()
                    .filter(|vertex| vertex.color[0] == vertex.color[1])
                    .map(|vertex| unmount.transform_point3(Vec3::from(vertex.position)).y)
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!((bar_top - 1.28).abs() < 1.0e-5, "{mount:?}: {bar_top}");
                let overlay: Vec<_> = builder
                    .solid
                    .iter()
                    .filter(|vertex| vertex.color[1] > 0.0 && vertex.color[0] == 0.0)
                    .map(|vertex| uncloth.transform_point3(Vec3::from(vertex.position)))
                    .collect();
                assert!(!overlay.is_empty());
                for point in overlay {
                    // Inflation separates artwork layers around the same cloth and hinge.
                    assert!((-0.401..=0.401).contains(&point.x));
                    assert!((-1.601..=0.001).contains(&point.y));
                    assert!((-0.081..=-0.039).contains(&point.z));
                }
            }
        }
    }
}

/// One patterned banner whose animation changes its geometry between publications.
fn patterned_banner() -> BannerModel {
    BannerModel {
        mount: BannerMount::Standing {
            rotation_degrees: 0.0,
        },
        base: [1.0, 0.0, 0.0],
        layers: std::iter::once(BannerLayer {
            pattern: "stripe_bottom",
            color: [0.0, 1.0, 0.0],
        })
        .collect(),
    }
}

#[test]
fn retained_banner_layers_clone_without_allocating() {
    let banner = patterned_banner();
    let (copy, allocations) = test_alloc::count(|| banner.clone());
    assert_eq!(allocations, 0);
    assert_eq!(copy, banner);
}

#[test]
fn warmed_banner_emission_reuses_texture_names() {
    let atlas = atlas(64);
    let model = patterned_banner();
    let mut builder = MeshBuilder::new(atlas.size());
    for ticks in [0.0, 1.0] {
        builder.reset(atlas.size());
        emit(&mut builder, &atlas, [0; 3], &model, SceneClock { ticks });
    }
    let (_, allocations) = test_alloc::count(|| {
        builder.reset(atlas.size());
        emit(
            &mut builder,
            &atlas,
            [0; 3],
            &model,
            SceneClock { ticks: 2.0 },
        );
    });
    assert_eq!(allocations, 0);
    let mut expected = MeshBuilder::new(atlas.size());
    emit(
        &mut expected,
        &atlas,
        [0; 3],
        &model,
        SceneClock { ticks: 2.0 },
    );
    assert_eq!(builder.solid, expected.solid);
    assert!(!builder.solid.is_empty());
}

#[test]
fn warmed_banner_publications_reuse_arrays_and_preserve_retained_frames() {
    use crate::{BlockEntityKind, BlockEntityScene, BlockEntitySubmission};
    use std::sync::Arc;

    let assets = artwork(crate::block_entity::MIN_DYNAMIC_ATLAS_WIDTH);
    let submissions = [BlockEntitySubmission {
        block: [0; 3],
        light: 1.0.into(),
        kind: BlockEntityKind::Banner(patterned_banner()),
    }];
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets);
    let held = scene
        .update(SceneClock { ticks: 0.0 }, &[], &submissions)
        .clone();
    let weak = Arc::downgrade(&held.solid);
    let original = bytemuck::cast_slice::<_, u8>(&held.solid).to_vec();
    for ticks in 1..10 {
        scene.update(
            SceneClock {
                ticks: f64::from(ticks),
            },
            &[],
            &submissions,
        );
    }
    let (frame, allocations) = test_alloc::count(|| {
        scene
            .update(SceneClock { ticks: 10.0 }, &[], &submissions)
            .clone()
    });
    assert_eq!(allocations, 0);
    assert!(!frame.solid.is_empty());
    assert_eq!(bytemuck::cast_slice::<_, u8>(&held.solid), original);
    assert!(Arc::ptr_eq(&weak.upgrade().unwrap(), &held.solid));
    let mut reference = BlockEntityScene::default();
    reference.install_assets(&assets);
    let expected = reference.update(SceneClock { ticks: 10.0 }, &[], &submissions);
    assert_eq!(frame.solid, expected.solid);
    assert_eq!(frame.overlay, expected.overlay);
    assert_eq!(frame.crack, expected.crack);
    assert_eq!(frame.portal, expected.portal);
    assert_eq!(frame.additive, expected.additive);
    assert_eq!(frame.dynamic_rgba8, expected.dynamic_rgba8);
}
