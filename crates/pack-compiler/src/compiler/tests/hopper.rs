use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

#[test]
fn hopper_carrier_keeps_materials_and_admits_every_backing_state() {
    let (records, lights, protocol) =
        super::shelf_cake::active_records(&["minecraft:hopper", "minecraft:stone"]);
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"outside"},"hopper":{"textures":{"down":"inside","up":"top","side":"outside"}}}"#,
    );
    let mut terrain = serde_json::json!({"texture_data": {}});
    for (name, color) in [
        ("inside", [40, 60, 80, 255]),
        ("top", [160, 180, 200, 255]),
        ("outside", [80, 100, 120, 255]),
    ] {
        let path = format!("textures/fixture/{name}");
        terrain["texture_data"][name] = serde_json::json!({"textures": path});
        write_png(
            directory.path().join(format!("{path}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &color.repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    write(
        directory.path().join("textures/terrain_texture.json"),
        terrain.to_string(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let facts = assets::facts::BlockEntityFacts::pinned();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:hopper")
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(visual.support(), VisualSupport::VanillaFallback);
        assert_eq!(
            runtime.resolve(NetworkIdMode::Hashed, record.network_hash),
            visual
        );
        let backing = facts.get(record.sequential_id).unwrap();
        assert!(
            backing.entity.geometry.accepts(visual.kind(), false),
            "hopper state {} must retain its known block-entity route",
            record.canonical_state
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 32);
        let mut materials: Vec<_> = quads.iter().map(|quad| quad.material).collect();
        materials.sort_unstable();
        materials.dedup();
        assert_eq!(
            materials.len(),
            3,
            "inside, rim and outside retain separate artwork"
        );
        assert!(!materials.contains(&assets::DIAGNOSTIC_MATERIAL));
        assert_ne!(
            visual.face(assets::BlockFace::Up).material_id(),
            visual.face(assets::BlockFace::Down).material_id()
        );
    }
}
