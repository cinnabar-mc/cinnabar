use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use pack_compile_input::pack::MISSING_TILE_TEXTURE_KEY;
use serde_json::{Value, json};

use super::*;

/// The network hash servers send for `minecraft:underwater_tnt`, which the registry knows only by
/// hash and the vanilla `blocks.json` leaves out.
const UNDERWATER_TNT_HASH: u32 = 0xd259_411e;
/// The fixture's `missing_tile` colour, distinct from the diagnostic texture.
const MISSING_TILE_PIXEL: [u8; 4] = [120, 80, 40, 255];

fn write_png(root: &Path, name: &str, pixel: [u8; 4]) {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&pixel.repeat(16 * 16), 16, 16, ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(root.join(format!("textures/blocks/{name}.png")), png).unwrap();
}

struct Fixture {
    compiled: CompiledAssets,
    hash_only: RegistryRecord,
    unlisted_named: RegistryRecord,
    pack: PackSources,
}

impl Fixture {
    /// Compiles a pack that lists dirt, and stone when `list_stone`, optionally defining the
    /// `missing_tile` texture key.
    fn compile(define_missing_tile: bool, list_stone: bool) -> Self {
        let target: Value =
            serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
                .unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
        let records: Vec<_> = assets::read_registry_for_protocol(
            &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
            protocol,
        )
        .unwrap()
        .into_iter()
        .filter(|record| {
            record.network_hash == UNDERWATER_TNT_HASH
                || matches!(
                    record.name.as_ref(),
                    "minecraft:stone" | "minecraft:dirt" | "minecraft:air"
                )
        })
        .collect();
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
        let mut blocks = json!({"dirt": {"textures": "dirt"}});
        if list_stone {
            blocks["stone"] = json!({"textures": "dirt"});
        }
        fs::write(
            directory.path().join("blocks.json"),
            serde_json::to_vec(&blocks).unwrap(),
        )
        .unwrap();
        let mut texture_data = json!({"dirt": {"textures": "textures/blocks/dirt"}});
        if define_missing_tile {
            texture_data[MISSING_TILE_TEXTURE_KEY] = json!({"textures": "textures/blocks/missing"});
        }
        fs::write(
            directory.path().join("textures/terrain_texture.json"),
            serde_json::to_vec(&json!({"texture_data": texture_data})).unwrap(),
        )
        .unwrap();
        fs::write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        )
        .unwrap();
        write_png(directory.path(), "dirt", [200, 200, 200, 255]);
        write_png(directory.path(), "missing", MISSING_TILE_PIXEL);
        let lights = vec![
            LightProperties::default();
            records
                .iter()
                .map(|record| record.sequential_id as usize + 1)
                .max()
                .unwrap()
        ];
        let (compiled, _) = compile_pack_inner(
            directory.path(),
            &records,
            &lights,
            CompiledBiomeAssets::diagnostic(),
            protocol,
        )
        .unwrap();
        let pack = read_pack(directory.path()).unwrap();
        let find = |wanted: &dyn Fn(&RegistryRecord) -> bool| {
            records
                .iter()
                .find(|record| wanted(record))
                .expect("the active registry has the fixture's block")
                .clone()
        };
        Self {
            compiled,
            hash_only: find(&|record| record.network_hash == UNDERWATER_TNT_HASH),
            unlisted_named: find(&|record| record.name.as_ref() == "minecraft:stone"),
            pack,
        }
    }

    fn assert_draws_the_missing_tile(&self, record: &RegistryRecord) {
        let visual = self.compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Cube);
        assert!(visual.flags.contains(BlockFlags::CUBE_GEOMETRY));
        for material in visual.faces {
            assert_ne!(material, DIAGNOSTIC_MATERIAL);
            let texture = self.compiled.materials[material as usize].texture;
            let mip = &self.compiled.texture_pages[texture.page() as usize]
                .texture
                .mips[0];
            let start = texture.layer() as usize * (mip.size * mip.size * 4) as usize;
            assert_eq!(mip.rgba8[start..start + 4], MISSING_TILE_PIXEL);
        }
    }
}

#[test]
fn a_block_known_only_by_hash_draws_the_missing_tile_on_every_face() {
    // Every named block has its own texture, so only the hash-only block needs the missing tile.
    let fixture = Fixture::compile(true, true);
    fixture.assert_draws_the_missing_tile(&fixture.hash_only);
}

#[test]
fn a_built_in_block_missing_from_blocks_json_draws_the_missing_tile_on_every_face() {
    let fixture = Fixture::compile(true, false);
    fixture.assert_draws_the_missing_tile(&fixture.unlisted_named);
}

#[test]
fn a_pack_without_the_missing_tile_keeps_the_diagnostic_texture() {
    let fixture = Fixture::compile(false, false);
    for record in [&fixture.hash_only, &fixture.unlisted_named] {
        let visual = fixture.compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.faces, [DIAGNOSTIC_MATERIAL; 6]);
    }
}

#[test]
fn a_custom_block_missing_from_blocks_json_keeps_the_diagnostic_texture() {
    let fixture = Fixture::compile(true, false);
    let mut custom = fixture.unlisted_named.clone();
    custom.name = "test:custom".into();
    let key = resolve_texture_key(&fixture.pack.blocks, &custom, BlockFace::Up);
    assert_eq!(key.key, None);
}
