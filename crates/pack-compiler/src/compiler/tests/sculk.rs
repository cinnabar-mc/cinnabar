use super::*;
use assets::{
    BlockFace, CompiledBiomeAssets, MODEL_QUAD_FLAG_FACE_MASK, MODEL_QUAD_FLAG_TWO_SIDED, ModelQuad,
};

const ART: [(&str, u8); 10] = [
    ("sculk_sensor_side", 10),
    ("sculk_sensor_bottom", 20),
    ("sculk_sensor_top", 30),
    ("sculk_sensor_tendril_inactive", 40),
    ("sculk_sensor_tendril_active", 50),
    ("sculk_shrieker_side", 60),
    ("sculk_shrieker_bottom", 70),
    ("sculk_shrieker_top", 80),
    ("sculk_shrieker_inner_top", 90),
    ("sculk_shrieker_can_summon_inner_top", 100),
];

fn marker(name: &str) -> u8 {
    ART.iter().find(|(art, _)| *art == name).unwrap().1
}

fn fixture() -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    fixture_with(serde_json::json!({
        "down": "sculk_sensor_bottom",
        "side": "sculk_sensor_side",
        "up": "sculk_sensor_top",
    }))
}

/// Compiles the sculk blocks with the given `textures` entry for the sensor.
fn fixture_with(
    sensor_textures: serde_json::Value,
) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air"
                    | "minecraft:stone"
                    | "minecraft:sculk_sensor"
                    | "minecraft:sculk_shrieker"
            )
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut texture_data = serde_json::json!({
        "stone": {"textures": "textures/blocks/stone"},
        "sculk_shrieker_inner_top": {"textures": [
            "textures/blocks/sculk_shrieker_inner_top",
            "textures/blocks/sculk_shrieker_can_summon_inner_top",
        ]},
    });
    for (name, red) in ART.iter().copied().chain([("stone", 1)]) {
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[red, 90, 40, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
        if !name.contains("inner_top") {
            texture_data[name] = serde_json::json!({"textures": format!("textures/blocks/{name}")});
        }
    }
    write(
        directory.path().join("blocks.json"),
        serde_json::json!({
            "stone": {"textures": "stone"},
            "sculk_sensor": {
                "carried_textures": {
                    "down": "sculk_sensor_tendril_inactive",
                    "side": "sculk_sensor_tendril_active",
                    "up": "sculk_sensor_tendril_inactive",
                },
                "textures": sensor_textures,
            },
            "sculk_shrieker": {
                "carried_textures": "sculk_shrieker_inner_top",
                "textures": {
                    "down": "sculk_shrieker_bottom",
                    "side": "sculk_shrieker_side",
                    "up": "sculk_shrieker_top",
                },
            },
        })
        .to_string(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::json!({"texture_data": texture_data}).to_string(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

/// The red channel naming the art a quad samples.
fn art(compiled: &assets::CompiledAssets, quad: &ModelQuad) -> u8 {
    let texture = compiled.materials[quad.material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize]
}

fn state_value(record: &RegistryRecord, property: &str) -> u64 {
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
    state[property]["value"].as_u64().unwrap()
}

fn quads<'a>(compiled: &'a assets::CompiledAssets, record: &RegistryRecord) -> &'a [ModelQuad] {
    let visual = compiled.visuals[record.sequential_id as usize];
    let template = compiled.model_templates[visual.model_template as usize];
    &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize]
}

/// The quad's lowest and highest coordinates on one axis.
fn span(quad: &ModelQuad, axis: usize) -> (i16, i16) {
    let values = quad.positions.map(|position| position[axis]);
    (
        values.into_iter().min().unwrap(),
        values.into_iter().max().unwrap(),
    )
}

#[test]
fn sculk_sensors_stand_phase_art_tendrils_on_a_half_height_body() {
    let (compiled, records) = fixture();
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:sculk_sensor")
    {
        let quads = quads(&compiled, record);
        assert_eq!(quads.len(), 10, "{}", record.canonical_state);
        let (body, tendrils) = quads.split_at(6);
        for quad in body {
            assert!(span(quad, 1).1 <= 128);
            let is_top = span(quad, 1) == (128, 128);
            assert_eq!(
                art(&compiled, quad) == marker("sculk_sensor_top"),
                is_top,
                "{}",
                record.canonical_state
            );
        }
        let tendril = if state_value(record, "sculk_sensor_phase") == 0 {
            marker("sculk_sensor_tendril_inactive")
        } else {
            marker("sculk_sensor_tendril_active")
        };
        let mut centres = Vec::new();
        for quad in tendrils {
            assert_eq!(art(&compiled, quad), tendril, "{}", record.canonical_state);
            assert_ne!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert_eq!(span(quad, 1), (128, 384));
            let (x, z) = (span(quad, 0), span(quad, 2));
            assert_eq!(x.1 - x.0, 184);
            assert_eq!(z.1 - z.0, 184);
            centres.push(((x.0 + x.1) / 2, (z.0 + z.1) / 2));
        }
        centres.sort_unstable();
        assert_eq!(centres, [(48, 48), (48, 208), (208, 48), (208, 208)]);
        seen += 1;
    }
    assert!(seen >= 3);
}

#[test]
fn sculk_shriekers_are_a_bowl_with_inner_walls_a_floor_and_a_rim() {
    let (compiled, records) = fixture();
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:sculk_shrieker")
    {
        let quads = quads(&compiled, record);
        assert_eq!(quads.len(), 20, "{}", record.canonical_state);
        let floor = if state_value(record, "can_summon") == 0 {
            marker("sculk_shrieker_inner_top")
        } else {
            marker("sculk_shrieker_can_summon_inner_top")
        };
        let side = marker("sculk_shrieker_side");
        let top = marker("sculk_shrieker_top");
        let walls = quads
            .iter()
            .filter(|quad| art(&compiled, quad) == side)
            .collect::<Vec<_>>();
        assert_eq!(walls.len(), 16);
        let culled = walls
            .iter()
            .filter(|quad| quad.flags >> 4 & MODEL_QUAD_FLAG_FACE_MASK != 0)
            .count();
        assert_eq!(culled, 8, "outer walls cull, inner walls do not");
        // Each inner wall plane stands inside the cell, facing the opposite way.
        let north_plane = walls
            .iter()
            .filter(|quad| span(quad, 2) == (32, 32))
            .count();
        assert_eq!(north_plane, 1);

        let floors = quads
            .iter()
            .filter(|quad| art(&compiled, quad) == floor)
            .collect::<Vec<_>>();
        assert_eq!(floors.len(), 1);
        assert_eq!(span(floors[0], 1), (128, 128));

        let rims = quads
            .iter()
            .filter(|quad| art(&compiled, quad) == top)
            .collect::<Vec<_>>();
        assert_eq!(rims.len(), 2);
        assert!(rims.iter().all(|quad| span(quad, 1) == (240, 240)));
        assert_eq!(
            rims.iter()
                .filter(|quad| span(quad, 0) == (16, 240))
                .count(),
            1,
            "the rim's upper face is inset, its under face is whole"
        );
        seen += 1;
    }
    assert!(seen >= 2);
}

/// A pack may give each side of the body its own art; the tendrils keep their carried art.
#[test]
fn sculk_sensor_bodies_keep_a_packs_art_for_each_face() {
    let (compiled, records) = fixture_with(serde_json::json!({
        "down": "sculk_sensor_bottom",
        "east": "sculk_shrieker_side",
        "north": "sculk_sensor_side",
        "south": "sculk_shrieker_bottom",
        "up": "sculk_sensor_top",
        "west": "sculk_shrieker_top",
    }));
    let record = records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:sculk_sensor"
                && state_value(record, "sculk_sensor_phase") == 0
        })
        .unwrap();
    let quads = quads(&compiled, record);
    for (face, name) in [
        (BlockFace::Down, "sculk_sensor_bottom"),
        (BlockFace::Up, "sculk_sensor_top"),
        (BlockFace::North, "sculk_sensor_side"),
        (BlockFace::South, "sculk_shrieker_bottom"),
        (BlockFace::West, "sculk_shrieker_top"),
        (BlockFace::East, "sculk_shrieker_side"),
    ] {
        assert_eq!(
            art(&compiled, &quads[face as usize]),
            marker(name),
            "{face:?}"
        );
    }
    assert_eq!(
        art(&compiled, &quads[6]),
        marker("sculk_sensor_tendril_inactive")
    );
}
