use super::*;

/// Parsed geometries keyed by source path and identifier.
type Geometries = BTreeMap<(Box<str>, Box<str>), PendingGeometry>;

/// Parses an original two-part fixture in the canonical villager geometry's frame.
fn fixture() -> (Vec<u8>, Geometries) {
    let value = serde_json::json!({
        "format_version": "1.21.0", "minecraft:geometry": [{
            "description": {"identifier": VILLAGER_GEOMETRY},
            "bones": [
                {"name": "head", "pivot": [0, 3, 0]},
                {"name": "brim", "parent": "head", "pivot": [0, 3, 0],
                    "cubes": [{"origin": [-2, 1, -1], "size": [4, 4, 1], "uv": [0, 0]}]}
            ]
        }]
    });
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut geometries = BTreeMap::new();
    super::super::geometry::parse_geometry(
        VILLAGER_PATH,
        std::path::Path::new(VILLAGER_PATH),
        &value,
        &mut BTreeMap::new(),
        &mut geometries,
    )
    .unwrap();
    (bytes, geometries)
}

#[test]
fn villager_brim_bind_rotates_only_the_plate_and_preserves_authored_binds() {
    let (_, mut geometries) = fixture();
    let key = (VILLAGER_GEOMETRY.into(), VILLAGER_PATH.into());
    let before = geometries[&key].bones.clone();
    restore_verified_geometry(&mut geometries, VILLAGER_PATH, VILLAGER_GEOMETRY);
    assert_eq!(geometries[&key].bones[0], before[0]);
    assert_eq!(
        geometries[&key].bones[1]
            .bind_pose_rotation
            .map(|rotation| rotation.map(EntityGeometryScalar::get)),
        Some([-90.0, 0.0, 0.0])
    );
    let authored = [10.0, 20.0, 30.0].map(|value| EntityGeometryScalar::new(value).unwrap());
    geometries.get_mut(&key).unwrap().bones[1].bind_pose_rotation = Some(authored);
    restore_verified_geometry(&mut geometries, VILLAGER_PATH, VILLAGER_GEOMETRY);
    assert_eq!(geometries[&key].bones[1].bind_pose_rotation, Some(authored));
}

#[test]
fn villager_brim_bind_leaves_unpinned_custom_sources_unchanged() {
    let (bytes, mut geometries) = fixture();
    let key = (VILLAGER_GEOMETRY.into(), VILLAGER_PATH.into());
    let before = geometries[&key].bones.clone();
    restore_sample_defaults(VILLAGER_PATH, &bytes, &mut geometries);
    assert_eq!(geometries[&key].bones, before);
}

#[test]
fn pinned_villager_brim_matches_the_shared_zombie_brim_bind() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ROOT").map(std::path::PathBuf::from) else {
        eprintln!("skipping missing fixture: CINNABAR_VANILLA_ROOT for villager brim bind");
        return;
    };
    let mut brims = Vec::new();
    for path in [VILLAGER_PATH, "models/entity/zombie_villager_v2.geo.json"] {
        let file = root.join(path);
        if !file.exists() {
            eprintln!(
                "skipping missing fixture: villager brim source {}",
                file.display()
            );
            return;
        }
        let bytes = std::fs::read(&file).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut geometries = BTreeMap::new();
        super::super::geometry::parse_geometry(
            path,
            &file,
            &value,
            &mut BTreeMap::new(),
            &mut geometries,
        )
        .unwrap();
        restore_sample_defaults(path, &bytes, &mut geometries);
        assert_eq!(geometries.len(), 1, "{path}");
        brims.push(
            geometries
                .into_values()
                .next()
                .unwrap()
                .bones
                .into_iter()
                .find(|bone| bone.name.as_ref() == "brim")
                .unwrap(),
        );
    }
    assert_eq!(brims[0].pivot, brims[1].pivot);
    assert_eq!(brims[0].cubes, brims[1].cubes);
    assert!(brims[1].bind_pose_rotation.is_some());
    assert_eq!(brims[0].bind_pose_rotation, brims[1].bind_pose_rotation);
}
