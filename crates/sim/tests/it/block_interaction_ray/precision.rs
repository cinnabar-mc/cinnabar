use super::*;

#[test]
fn click_intercept_uses_the_rounded_world_segment() {
    let (mut store, _) = loaded_air_store(ChunkKey::new(0, 524_288, 0));
    set_block(&mut store, [8_388_609, 1, 0], 0, 7);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let origin = Vec3::new(8_388_608.0, 0.5, 0.5);
    // X ends one block from the origin after rounding; Y travels 1.6 blocks.
    // The rounded segment passes above the target before reaching its west face.
    assert!(
        world
            .block_interaction_ray_current(origin, Vec3::new(0.6, 0.8, 0.0), 2.0)
            .unwrap()
            .is_none()
    );
    assert!(
        world
            .camera_visibility_ray(origin, Vec3::new(0.6, 0.8, 0.0), 2.0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn block_hit_distance_is_a_widened_float_square_root() {
    let (store, registry, _) = fixture(7);
    let origin = Vec3::new(-1.0, 0.5, -1.0);
    let hit = PaletteWorld::new(&store, &registry, 0, Default::default())
        .block_interaction_ray_current(origin, Vec3::new(1.0, 0.0, 1.0), 4.0)
        .unwrap()
        .unwrap();
    assert_eq!(hit.hit_local, Vec3::new(0.0, 0.5, 0.0));
    // The float intercept interpolates every axis, so it lands one ulp short of the corner.
    let [x, y, z] = [
        hit.hit_world.x as f32 - origin.x as f32,
        hit.hit_world.y as f32 - origin.y as f32,
        hit.hit_world.z as f32 - origin.z as f32,
    ];
    assert!((x - 1.0).abs() <= f32::EPSILON && y == 0.0 && (z - 1.0).abs() <= f32::EPSILON);
    assert_eq!(hit.distance, f64::from((z * z + y * y + x * x).sqrt()));
    let [x, y, z] = [f64::from(x), f64::from(y), f64::from(z)];
    assert_ne!(hit.distance, (z * z + y * y + x * x).sqrt());
}

#[test]
fn endpoint_cells_remain_reachable_after_many_crossings() {
    use sim::CollisionWorld;

    for axis in 0..3 {
        let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
        let mut block = [0; 3];
        block[axis] = 10;
        set_block(&mut store, block, 0, 7);
        let mut registry = CollisionRegistry::with_identity(identity());
        registry.register(0, []).unwrap();
        registry
            .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
            .unwrap();
        let expected = expected(&store, &chunks);
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        let mut origin = Vec3::new(0.5, 0.5, 0.5);
        origin[axis] = 0.0;
        let mut direction = Vec3::ZERO;
        direction[axis] = 1.0;
        let delta = direction * 10.0;

        let hit = world
            .block_interaction_ray(origin, direction, 10.0, &expected)
            .unwrap()
            .unwrap();
        assert_eq!(hit.block_pos, block);
        assert_eq!(hit.distance, 10.0);
        let visible = world
            .camera_visibility_ray(origin, direction, 10.0)
            .unwrap()
            .unwrap();
        assert_eq!(visible.block_pos, block);
        assert_eq!(visible.distance, hit.distance);
        assert_eq!(
            world.camera_segment_entry(origin, delta).unwrap().0,
            Some(1.0)
        );
        for outlines in [false, true] {
            let aimed = world
                .camera_aim_ray(origin, origin + delta, 10, outlines, false)
                .unwrap()
                .unwrap();
            assert_eq!(aimed.block_pos, block);
            assert_eq!(aimed.distance, hit.distance);
        }
    }
}

#[test]
fn rounded_zero_motion_in_a_face_plane_meets_the_face_it_crosses() {
    let (mut store, _) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [10, 0, 0], 0, 7);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());

    // On the low face plane the walked cell column holds the block, and closed face bounds admit
    // the crossing; on the high plane the walk stays in the neighbouring column.
    for (x, dx, hits) in [(10.0, -1.0e-8, true), (11.0, 1.0e-8, false)] {
        let origin = Vec3::new(x, 0.5, -1.0);
        let direction = Vec3::new(dx, 0.0, 1.0);
        let hit = world
            .block_interaction_ray_current(origin, direction, 3.0)
            .unwrap();
        assert_eq!(
            hit.map(|hit| (hit.block_pos, hit.distance)),
            hits.then_some(([10, 0, 0], 1.0))
        );
        let visible = world.camera_visibility_ray(origin, direction, 3.0).unwrap();
        assert_eq!(visible.map(|hit| hit.block_pos), hits.then_some([10, 0, 0]));
        for outlines in [false, true] {
            let aimed = world
                .camera_aim_ray(origin, origin + direction * 3.0, 3, outlines, false)
                .unwrap();
            assert_eq!(aimed.map(|hit| hit.block_pos), hits.then_some([10, 0, 0]));
        }
    }
}

/// A stored corner intercept remains the exact closed endpoint when the world is refreshed.
#[test]
fn refreshing_a_saved_intercept_does_not_rebuild_its_endpoint() {
    let (store, registry, _) = fixture(7);
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let endpoint = Vec3::new(0.0, 0.5, 0.0);
    let hit = world
        .block_interaction_segment_current(Vec3::new(-1.0, 0.5, -1.0), endpoint)
        .unwrap()
        .unwrap();
    assert_eq!(hit.block_pos, [0, 0, 0]);
    assert_eq!(hit.hit_world, endpoint);
    assert!(
        world
            .block_interaction_segment_current(endpoint, endpoint)
            .unwrap()
            .is_none()
    );
}

/// A shorter traversal must not rotate a rounded ray into an otherwise missed corner.
#[test]
fn traversal_prefix_preserves_the_full_rounded_ray_at_large_corners() {
    let (mut store, _) = loaded_air_store(ChunkKey::new(0, 512, 256));
    set_block(&mut store, [8193, 0, 4098], 0, 7);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let origin = Vec3::new(8_192.195_312_5, 0.5, 4096.5);
    let direction = Vec3::new(
        f64::from(0.306_398_66_f32),
        0.0,
        f64::from(0.951_903_34_f32),
    );
    assert!(
        world
            .block_interaction_ray_current(origin, direction, 5.7)
            .unwrap()
            .is_none()
    );
    assert!(
        world
            .block_interaction_ray_current(origin, direction, 2.73)
            .unwrap()
            .is_some()
    );
    assert!(
        world
            .block_interaction_ray_prefix_current(origin, direction, 5.7, 2.73)
            .unwrap()
            .is_none()
    );
}

/// Prefixes retain every in-range full-ray hit and exclude only farther geometry.
#[test]
fn traversal_prefix_matches_the_full_ray_through_boundary_samples() {
    let (mut store, _) = loaded_air_store(ChunkKey::new(0, 512, 256));
    set_block(&mut store, [8193, 0, 4098], 0, 7);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let origin = Vec3::new(8_192.195_312_5, 0.5, 4096.5);
    for x_offset in [-0.1_f32, -0.01, -0.001, 0.0, 0.001, 0.01, 0.1] {
        let x = 0.306_398_66_f32 + x_offset;
        let direction = Vec3::new(f64::from(x), 0.0, f64::from(0.951_903_34_f32));
        let full = world
            .block_interaction_ray_current(origin, direction, 5.7)
            .unwrap();
        for prefix in [0.0, 1.0, 2.0, 2.625, 2.73, 3.0, 5.7, 9.0] {
            let bounded = world
                .block_interaction_ray_prefix_current(origin, direction, 5.7, prefix)
                .unwrap();
            let expected = full.clone().filter(|hit| hit.distance <= prefix);
            let geometry =
                |hit: sim::BlockHit| (hit.block_pos, hit.face, hit.hit_world, hit.distance);
            assert_eq!(
                bounded.map(geometry),
                expected.map(geometry),
                "direction {direction:?}, prefix {prefix}"
            );
        }
    }
}
