use super::*;
use assets::{BlockFace, CompiledBiomeAssets, ModelQuad, NetworkIdMode, RuntimeAssets};

/// Compiles the admitted crop states against distinct original directional stage art.
fn fixture() -> (RuntimeAssets, Vec<RegistryRecord>) {
    let protocol = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(
        include_bytes!("../../../../../crates/assets/data/block-registry-v2225.bin"),
        protocol,
    )
    .unwrap()
    .into_vec()
    .into_iter()
    .filter(|r| {
        matches!(
            r.name.as_ref(),
            "minecraft:air"
                | "minecraft:melon_stem"
                | "minecraft:pumpkin_stem"
                | "minecraft:pitcher_crop"
                | "minecraft:torchflower_crop"
                | "minecraft:torchflower"
                | "minecraft:sweet_berry_bush"
                | "minecraft:wheat"
        )
    })
    .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let mut blocks = serde_json::json!({
        "sweet_berry_bush":{"textures":{
            "down":"sweet_berry_bush_0", "up":"sweet_berry_bush_1",
            "north":"sweet_berry_bush_2", "south":"sweet_berry_bush_3",
            "west":"sweet_berry_bush_3", "east":"sweet_berry_bush_3"
        }},
        "pitcher_crop":{"textures":{
            "down":"pitcher_crop_bottom", "up":"pitcher_crop_top",
            "north":"pitcher_crop_side", "south":"pitcher_crop_lower_flower",
            "west":"pitcher_crop_upper_flower", "east":"pitcher_crop_upper_flower"
        }}
    });
    let mut terrain = serde_json::Map::new();
    for (key, count, tag) in [
        ("melon_stem", 2, 20),
        ("pumpkin_stem", 2, 30),
        ("pitcher_crop_bottom", 1, 40),
        ("pitcher_crop_top", 1, 50),
        ("pitcher_crop_side", 1, 60),
        ("pitcher_crop_lower_flower", 4, 70),
        ("pitcher_crop_upper_flower", 2, 80),
        ("torchflower_crop", 2, 90),
        ("torchflower", 1, 100),
        ("wheat", 8, 110),
        ("sweet_berry_bush_0", 1, 120),
        ("sweet_berry_bush_1", 1, 121),
        ("sweet_berry_bush_2", 1, 122),
        ("sweet_berry_bush_3", 1, 123),
    ] {
        if !key.starts_with("pitcher_") && !key.starts_with("sweet_berry_bush_") {
            blocks[key] = serde_json::json!({"textures":key});
        }
        let paths = (0..count)
            .map(|index| {
                let path = format!("textures/{key}_{index}");
                let pixels = (0..TILE_SIZE)
                    .flat_map(|y| {
                        (0..TILE_SIZE).flat_map(move |x| {
                            // The connected stem's branch occupies only the left half.
                            let alpha =
                                if key.ends_with("_stem") && index == 1 && x >= TILE_SIZE / 2 {
                                    0
                                } else {
                                    255
                                };
                            [tag + index as u8, x as u8 * 4, y as u8 * 4, alpha]
                        })
                    })
                    .collect::<Vec<_>>();
                write_png(
                    directory.path().join(format!("{path}.png")),
                    TILE_SIZE,
                    TILE_SIZE,
                    &pixels,
                );
                path
            })
            .collect::<Vec<_>>();
        terrain.insert(key.into(), serde_json::json!({"textures":paths}));
    }
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":terrain})).unwrap(),
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
        &vec![assets::LightProperties::default(); span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (
        RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap(),
        records,
    )
}

/// Reads a typed registry property from the fixture's actual state product.
fn property(record: &RegistryRecord, key: &str) -> u32 {
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
    state[key]["value"].as_u64().unwrap() as u32
}

/// Resolves one model in either supported network-ID mode.
fn quads<'a>(
    runtime: &'a RuntimeAssets,
    record: &RegistryRecord,
    mode: NetworkIdMode,
) -> &'a [ModelQuad] {
    let id = match mode {
        NetworkIdMode::Sequential => record.sequential_id,
        NetworkIdMode::Hashed => record.network_hash,
    };
    let visual = runtime.resolve(mode, id);
    assert_eq!(
        visual.support(),
        VisualSupport::Exact,
        "{} {}",
        record.name,
        record.canonical_state
    );
    if record.name.as_ref() == "minecraft:pitcher_crop" || record.name.ends_with("_stem") {
        assert_eq!(
            assets::bamboo::transform_for_template(0, visual.variant(), [0; 3]),
            0,
            "preoriented crop geometry keeps its world pose"
        );
    }
    let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
    &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize]
}

/// Identifies the original sprite without relying on compiler material numbering.
fn art(runtime: &RuntimeAssets, material: u32) -> u8 {
    sprite_pixel(runtime, material, [0, 0])[0]
}

/// Reads a fixture sprite pixel at normalized texture coordinates in 1/4096 units.
fn sprite_pixel(runtime: &RuntimeAssets, material: u32, uv: [u16; 2]) -> [u8; 4] {
    let texture = runtime.materials()[material as usize].texture;
    let mip = &runtime.texture_pages()[texture.page() as usize]
        .texture
        .mips[0];
    let offset = texture.layer() as usize * (mip.size * mip.size * 4) as usize;
    let [x, y] =
        uv.map(|value| (usize::from(value) * mip.size as usize / 4096).min(mip.size as usize - 1));
    let pixel = offset + (y * mip.size as usize + x) * 4;
    mip.rgba8[pixel..pixel + 4].try_into().unwrap()
}

#[test]
fn stems_grow_from_the_soil_and_keep_their_upright_sprite_when_attached() {
    let (runtime, records) = fixture();
    let mut count = 0;
    let mut growth_materials = BTreeMap::<Box<str>, std::collections::BTreeSet<u32>>::new();
    for record in records.iter().filter(|r| r.name.ends_with("_stem")) {
        count += 1;
        let growth = property(record, "growth");
        let facing = property(record, "facing_direction");
        let tag = if record.name.as_ref() == "minecraft:melon_stem" {
            20
        } else {
            30
        };
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let id = if mode == NetworkIdMode::Sequential {
                record.sequential_id
            } else {
                record.network_hash
            };
            assert_eq!(runtime.resolve(mode, id).kind(), VisualKind::Cross);
            let model = quads(&runtime, record, mode);
            assert_eq!(
                model.len(),
                if facing == 0 { 2 } else { 3 },
                "{} {}",
                record.name,
                record.canonical_state
            );
            let height = if facing == 0 {
                (growth + 1) as i16 * 32
            } else {
                128
            };
            for quad in &model[..2] {
                assert_eq!(
                    art(&runtime, quad.material),
                    tag,
                    "upright stem uses disconnected art"
                );
                assert_eq!(
                    assets::stem::material_colour(
                        runtime.materials()[quad.material as usize].flags
                    ),
                    assets::stem::colour(growth)
                );
                assert_eq!(quad.positions.iter().map(|p| p[1]).min(), Some(-16));
                assert_eq!(quad.positions.iter().map(|p| p[1]).max(), Some(height - 16));
                for (position, uv) in quad.positions.iter().zip(quad.uvs) {
                    assert_eq!(
                        uv[1],
                        if position[1] == -16 {
                            height as u16 * 16
                        } else {
                            0
                        }
                    );
                }
            }
            if facing != 0 {
                let connector = &model[2];
                assert_eq!(art(&runtime, connector.material), tag + 1);
                let axis = if matches!(facing, 4 | 5) { 2 } else { 0 };
                assert!(connector.positions.iter().all(|p| p[axis] == 128));
                assert_eq!(connector.positions.iter().map(|p| p[1]).min(), Some(-16));
                assert_eq!(connector.positions.iter().map(|p| p[1]).max(), Some(240));
            } else {
                growth_materials
                    .entry(record.name.clone())
                    .or_default()
                    .insert(model[0].material);
            }
        }
    }
    assert_eq!(count, 96);
    assert!(
        growth_materials
            .values()
            .all(|materials| materials.len() == 8),
        "each growth stage has its own colour"
    );
}

#[test]
fn attached_stem_branches_point_toward_the_fruit() {
    let (runtime, records) = fixture();
    let mut count = 0;
    for record in records
        .iter()
        .filter(|record| record.name.ends_with("_stem"))
    {
        let (axis, fruit_edge) = match property(record, "facing_direction") {
            2 => (2, 0),   // North.
            3 => (2, 256), // South.
            4 => (0, 0),   // West.
            5 => (0, 256), // East.
            _ => continue,
        };
        count += 1;
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let connector = &quads(&runtime, record, mode)[2];
            let (fruit, away) = if connector.positions[1][axis] == fruit_edge {
                (1, 2)
            } else {
                (2, 1)
            };
            assert_eq!(connector.positions[fruit][axis], fruit_edge);
            // Sample halfway from the stem centre toward each world-space endpoint.
            let fruit_u = (3 * connector.uvs[fruit][0] + connector.uvs[away][0]) / 4;
            let away_u = (connector.uvs[fruit][0] + 3 * connector.uvs[away][0]) / 4;
            assert_eq!(
                sprite_pixel(&runtime, connector.material, [fruit_u, 2048])[3],
                255,
                "branch must face the fruit: {} {} {mode:?}",
                record.name,
                record.canonical_state
            );
            assert_eq!(
                sprite_pixel(&runtime, connector.material, [away_u, 2048])[3],
                0,
                "opposite side must be transparent: {} {} {mode:?}",
                record.name,
                record.canonical_state
            );
        }
    }
    assert_eq!(count, 64);
}

#[test]
fn pitcher_pods_and_flowers_keep_plant_semantics_and_distinct_art() {
    let (runtime, records) = fixture();
    let mut count = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:pitcher_crop")
    {
        count += 1;
        let growth = property(record, "growth");
        let upper = property(record, "upper_block_bit") != 0;
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let id = if mode == NetworkIdMode::Sequential {
                record.sequential_id
            } else {
                record.network_hash
            };
            if upper && growth < 3 {
                assert_eq!(runtime.resolve(mode, id).kind(), VisualKind::Invisible);
                continue;
            }
            assert_eq!(runtime.resolve(mode, id).kind(), VisualKind::Cross);
            let model = quads(&runtime, record, mode);
            if upper {
                assert_eq!(model.len(), 2);
                for axis in [0, 2] {
                    assert_eq!(
                        model
                            .iter()
                            .flat_map(|q| q.positions)
                            .map(|p| p[axis])
                            .min(),
                        Some(36)
                    );
                    assert_eq!(
                        model
                            .iter()
                            .flat_map(|q| q.positions)
                            .map(|p| p[axis])
                            .max(),
                        Some(220)
                    );
                }
                assert!(
                    model
                        .iter()
                        .all(|q| art(&runtime, q.material) == 80 + u8::from(growth != 3))
                );
                assert_eq!(
                    model.iter().flat_map(|q| q.positions).map(|p| p[1]).min(),
                    Some(0)
                );
            } else {
                assert_eq!(model.len(), if growth == 0 { 6 } else { 8 });
                let pod = &model[..6];
                for (face, quad) in BlockFace::ALL.into_iter().zip(pod) {
                    let expected = match face {
                        BlockFace::Down => 40,
                        BlockFace::Up => 50,
                        _ => 60,
                    };
                    assert_eq!(art(&runtime, quad.material), expected);
                    if !matches!(face, BlockFace::Down | BlockFace::Up) {
                        assert_eq!(quad.uvs.iter().map(|uv| uv[0]).min(), Some(768));
                        assert_eq!(
                            quad.uvs.iter().map(|uv| uv[0]).max(),
                            Some(if growth == 0 { 2304 } else { 3328 })
                        );
                        assert_eq!(quad.uvs.iter().map(|uv| uv[1]).min(), Some(2560));
                        assert_eq!(
                            quad.uvs.iter().map(|uv| uv[1]).max(),
                            Some(if growth == 0 { 3584 } else { 4096 })
                        );
                    }
                }
                assert_eq!(
                    pod.iter().flat_map(|q| q.positions).map(|p| p[1]).min(),
                    Some(-16)
                );
                assert_eq!(
                    pod.iter().flat_map(|q| q.positions).map(|p| p[1]).max(),
                    Some(if growth == 0 { 48 } else { 80 })
                );
                if growth > 0 {
                    for axis in [0, 2] {
                        assert_eq!(
                            model[6..]
                                .iter()
                                .flat_map(|q| q.positions)
                                .map(|p| p[axis])
                                .min(),
                            Some(36)
                        );
                        assert_eq!(
                            model[6..]
                                .iter()
                                .flat_map(|q| q.positions)
                                .map(|p| p[axis])
                                .max(),
                            Some(220)
                        );
                    }
                    assert!(
                        model[6..]
                            .iter()
                            .all(|q| art(&runtime, q.material) == 70 + growth.min(4) as u8 - 1)
                    );
                    assert_eq!(
                        model[6..]
                            .iter()
                            .flat_map(|q| q.positions)
                            .map(|p| p[1])
                            .min(),
                        Some(if growth <= 2 { 80 } else { 0 })
                    );
                }
            }
        }
    }
    assert_eq!(count, 16);
}

#[test]
fn torchflower_stages_and_common_crop_control_keep_their_art() {
    let (runtime, records) = fixture();
    let mut torchflowers = 0;
    for record in &records {
        let name = record.name.as_ref();
        if !matches!(
            name,
            "minecraft:torchflower_crop" | "minecraft:torchflower" | "minecraft:wheat"
        ) {
            continue;
        }
        let tag = match name {
            "minecraft:torchflower_crop" => {
                torchflowers += 1;
                90 + u8::from(property(record, "growth") >= 4)
            }
            "minecraft:torchflower" => 100,
            _ => 110 + property(record, "growth") as u8,
        };
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let model = quads(&runtime, record, mode);
            assert_eq!(model.len(), if name == "minecraft:wheat" { 4 } else { 2 });
            assert!(model.iter().all(|q| art(&runtime, q.material) == tag));
            if name != "minecraft:wheat" {
                for axis in [0, 2] {
                    assert_eq!(
                        model
                            .iter()
                            .flat_map(|q| q.positions)
                            .map(|p| p[axis])
                            .min(),
                        Some(13)
                    );
                    assert_eq!(
                        model
                            .iter()
                            .flat_map(|q| q.positions)
                            .map(|p| p[axis])
                            .max(),
                        Some(243)
                    );
                }
            }
        }
    }
    assert_eq!(torchflowers, 8);
}

#[test]
fn berry_bush_growth_selects_each_stage_face_and_preserves_mature_art() {
    let (runtime, records) = fixture();
    let mut states = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:sweet_berry_bush")
    {
        states += 1;
        let growth = property(record, "growth");
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let model = quads(&runtime, record, mode);
            assert_eq!(model.len(), 2);
            assert!(
                model
                    .iter()
                    .all(|q| art(&runtime, q.material) == 120 + growth.min(3) as u8),
                "growth {growth} must use its stage art"
            );
            for axis in [0, 2] {
                assert_eq!(
                    model
                        .iter()
                        .flat_map(|q| q.positions)
                        .map(|p| p[axis])
                        .min(),
                    Some(36)
                );
                assert_eq!(
                    model
                        .iter()
                        .flat_map(|q| q.positions)
                        .map(|p| p[axis])
                        .max(),
                    Some(220)
                );
            }
        }
    }
    assert_eq!(states, 8);
}
