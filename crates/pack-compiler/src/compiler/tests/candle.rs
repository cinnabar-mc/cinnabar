use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Native candle layouts per zero-based count: height in pixels and centre in 1/256 blocks.
const NATIVE_CANDLES: [&[(i16, [i16; 2])]; 4] = [
    &[(6, [128, 128])],
    &[(6, [160, 128]), (5, [96, 144])],
    &[(6, [144, 112]), (5, [96, 128]), (3, [128, 160])],
    &[
        (6, [144, 96]),
        (5, [96, 96]),
        (5, [160, 144]),
        (3, [112, 144]),
    ],
];

/// Every placed candle identity with an original pack giving each colour its own unlit and
/// lit sprite: red is the colour index, green separates lit from unlit.
fn fixture(root: &Path) -> (Vec<RegistryRecord>, RuntimeAssets) {
    let (all, lights, protocol) = super::shelf_cake::active_records(&[]);
    let records = all
        .into_iter()
        .filter(|record| {
            matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
                || assets::facts::block_render_fact(&record.name).kind
                    == assets::facts::BlockRenderKind::Candle
        })
        .collect::<Vec<_>>();
    let mut blocks = serde_json::json!({"stone":{"textures":"stone"}});
    let mut terrain = serde_json::json!({"texture_data":{"stone":{"textures":"textures/stone"}}});
    write_png(
        root.join("textures/stone.png"),
        16,
        16,
        &[100, 100, 100, 255].repeat(256),
    );
    for (index, name) in candle_names(&records).into_iter().enumerate() {
        let name = name.strip_prefix("minecraft:").unwrap();
        blocks[name] = serde_json::json!({"textures": name});
        terrain["texture_data"][name] = serde_json::json!({"textures": [
            format!("textures/{name}"),
            format!("textures/{name}_lit"),
        ]});
        for (suffix, green) in [("", 90), ("_lit", 180)] {
            write_png(
                root.join(format!("textures/{name}{suffix}.png")),
                16,
                16,
                &[index as u8 * 10, green, 60, 255].repeat(256),
            );
        }
    }
    write(
        root.join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    write(
        root.join("textures/terrain_texture.json"),
        serde_json::to_vec(&terrain).unwrap(),
    );
    write(root.join("textures/flipbook_textures.json"), "[]");
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        root,
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    (records, runtime)
}

fn candle_names(records: &[RegistryRecord]) -> Vec<&str> {
    let mut names: Vec<_> = records
        .iter()
        .filter(|record| record.name.ends_with("candle"))
        .map(|record| record.name.as_ref())
        .collect();
    names.dedup();
    names
}

fn state(record: &RegistryRecord) -> (usize, bool) {
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
    (
        state["candles"]["value"].as_u64().unwrap() as usize,
        state["lit"]["value"].as_u64().unwrap() != 0,
    )
}

#[test]
fn every_candle_count_colour_and_lit_state_draws_the_native_candles() {
    let directory = tempfile::tempdir().unwrap();
    let (records, runtime) = fixture(directory.path());
    let names = candle_names(&records);
    assert_eq!(names.len(), 17, "the undyed candle and sixteen dyes");
    let candles: Vec<_> = records
        .iter()
        .filter(|record| record.name.ends_with("candle"))
        .collect();
    assert_eq!(candles.len(), 17 * 4 * 2);
    let pixels = &runtime.texture_array().mips[0].rgba8;
    for record in candles {
        let (count, lit) = state(record);
        let label = format!("{} {}", record.name, record.canonical_state);
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model, "{label}");
        assert_eq!(visual.support(), VisualSupport::Exact, "{label}");
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        let parts = assets::model_template_parts(
            runtime.model_templates(),
            visual.model_template().unwrap(),
        )
        .unwrap();
        // A candle and its wick never straddle two template parts.
        assert!(
            parts.iter().all(|part| part.quad_count % 10 == 0),
            "{label}"
        );
        let quad_count: usize = parts.iter().map(|part| part.quad_count as usize).sum();
        let quads = &runtime.model_quads()[parts[0].quad_start as usize..][..quad_count];
        let layout = NATIVE_CANDLES[count];
        assert_eq!(quads.len(), layout.len() * 10, "{label}");
        let material = quads[0].material;
        assert!(quads.iter().all(|quad| quad.material == material));
        let texture = runtime.material(material).texture;
        let texel = (texture.layer() as usize * TILE_SIZE as usize * TILE_SIZE as usize) * 4;
        let index = names.iter().position(|name| *name == record.name.as_ref());
        assert_eq!(
            pixels[texel..texel + 3],
            [index.unwrap() as u8 * 10, if lit { 180 } else { 90 }, 60],
            "{label}: colour and lit sprite"
        );
        assert_ne!(
            runtime.material(material).flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT,
            0
        );
        for (candle, &(height, [x, z])) in quads.chunks(10).zip(layout) {
            let bounds: [[i16; 2]; 3] = std::array::from_fn(|axis| {
                let values = candle[..6]
                    .iter()
                    .flat_map(|q| q.positions)
                    .map(|p| p[axis]);
                [values.clone().min().unwrap(), values.max().unwrap()]
            });
            assert_eq!(
                bounds,
                [[x - 16, x + 16], [0, height * 16], [z - 16, z + 16]],
                "{label}"
            );
            for (face, quad) in assets::BlockFace::ALL.into_iter().zip(&candle[..6]) {
                assert_eq!(quad.flags & assets::MODEL_QUAD_FLAG_INHERITED_SHADE, 0);
                let rows = match face {
                    assets::BlockFace::Down => [14, 16],
                    assets::BlockFace::Up => [6, 8],
                    _ => [8, 8 + height as u16],
                };
                let v = quad.uvs.map(|uv| uv[1]);
                assert_eq!(
                    [*v.iter().min().unwrap(), *v.iter().max().unwrap()],
                    rows.map(|row| row * 256),
                    "{label} {face:?}"
                );
            }
            let wick = &candle[6..];
            assert!(wick.iter().all(|quad| {
                quad.flags & assets::MODEL_QUAD_FLAG_INHERITED_SHADE != 0
                    && quad.positions.iter().all(|p| {
                        (p[0] - x).abs() == 8
                            && (p[2] - z).abs() == 8
                            && [height * 16, height * 16 + 16].contains(&p[1])
                    })
            }));
        }
    }
}
