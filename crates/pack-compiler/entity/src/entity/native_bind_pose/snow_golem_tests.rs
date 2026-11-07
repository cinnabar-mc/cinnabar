use super::*;

const PATH: &str = "models/entity/snow_golem.geo.json";
const GEOMETRY: &str = "geometry.snowgolem.v1.8";

#[test]
fn snow_golem_repair_requires_review_when_the_sample_pin_changes() {
    assert_eq!(assets::active_content_registry_protocol(), 2225);
    assert_eq!(
        assets::vanilla_source().commit.as_ref(),
        "4d30ae63750467771b6eccda105f732c6cb77ff7"
    );
}

fn parse(bytes: &[u8]) -> BTreeMap<(Box<str>, Box<str>), PendingGeometry> {
    let mut geometries = BTreeMap::new();
    super::super::geometry::parse_geometry(
        PATH,
        std::path::Path::new(PATH),
        &serde_json::from_slice(bytes).unwrap(),
        &mut BTreeMap::new(),
        &mut geometries,
    )
    .unwrap();
    geometries
}

fn expected(name: &str) -> Option<[EntityGeometryScalar; 3]> {
    match name {
        "arm1" => Some([0.0, 0.0, 57.3]),
        "arm2" => Some([0.0, 180.0, -57.3]),
        _ => None,
    }
    .map(|v| v.map(|v| EntityGeometryScalar::new(v).unwrap()))
}

#[test]
fn snow_golem_cube_binds_separate_arms_without_changing_animation_frames() {
    let mut geometries = parse(
        br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.snowgolem.v1.8"},"bones":[
          {"name":"body","pivot":[0,8,0]},
          {"name":"arm1","parent":"body","pivot":[0,16,0],"rotation":[2,3,4]},
          {"name":"arm2","parent":"body","pivot":[0,16,0]},
          {"name":"held","parent":"arm1","pivot":[2,19,0]}
        ]}]}"#,
    );
    let before = geometries.clone();
    restore_verified_geometry(&mut geometries, PATH, GEOMETRY);
    for (key, old) in &before {
        for (old, new) in old.bones.iter().zip(&geometries[key].bones) {
            let mut wanted = old.clone();
            wanted.bind_pose_rotation = expected(&old.name);
            assert_eq!(&wanted, new, "{}", old.name);
        }
    }
    let arm = &mut geometries.values_mut().next().unwrap().bones[1];
    arm.bind_pose_rotation = Some([EntityGeometryScalar::ZERO; 3]);
    restore_verified_geometry(&mut geometries, PATH, GEOMETRY);
    assert_eq!(
        geometries.values().next().unwrap().bones[1].bind_pose_rotation,
        Some([EntityGeometryScalar::ZERO; 3])
    );
}

#[test]
fn pinned_snow_golem_restores_only_the_missing_retail_cube_binds() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ROOT") else {
        eprintln!("skipping missing fixture: CINNABAR_VANILLA_ROOT is not set");
        return;
    };
    let path = std::path::PathBuf::from(root).join(PATH);
    if !path.exists() {
        eprintln!("skipping missing fixture: {}", path.display());
        return;
    }
    let bytes = std::fs::read(path).unwrap();
    let mut geometries = parse(&bytes);
    let before = geometries.clone();
    let mut changed = bytes.clone();
    changed.push(b'\n');
    restore_sample_defaults(PATH, &changed, &mut geometries);
    for (key, old) in &before {
        assert_eq!(old.bones, geometries[key].bones);
    }
    restore_sample_defaults(PATH, &bytes, &mut geometries);
    for (key, old) in &before {
        for (old, new) in old.bones.iter().zip(&geometries[key].bones) {
            let mut wanted = old.clone();
            wanted.bind_pose_rotation = expected(&old.name);
            assert_eq!(&wanted, new, "{}", old.name);
        }
    }
}
