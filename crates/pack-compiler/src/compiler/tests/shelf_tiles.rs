//! Shelves against the region table of native declarative shelf tessellation.

use super::shelf_cake::active_records;
use super::*;
use assets::material_keys::MaterialKeys;
use assets::{
    BlockFace, CompiledAssets, CompiledBiomeAssets, MODEL_TEMPLATE_FLAG_SHEET_TILES,
    MODEL_TEMPLATE_FLAG_SHELF, ModelQuad, RuntimeAssets,
};

const SHELF: &str = "minecraft:poplar_shelf";
/// Native shelf regions are authored in a sheet of this many texels per side.
const NATIVE_SHEET: u32 = 32;

/// Sheet rectangles per emitted quad in the south pose, written `[u0, v0, u1, v1]`: west, east,
/// down, up and north of the back, then west, east, down, up and south of each ledge. The panel
/// follows, chosen from [`NATIVE_FRONTS`]. A reversed pair is a mirrored face.
const NATIVE_REGIONS: [[u16; 4]; 15] = [
    [16, 0, 19, 16],
    [29, 0, 32, 16],
    [32, 12, 16, 9],
    [32, 10, 16, 7],
    [16, 0, 32, 16],
    [3, 0, 5, 4],
    [11, 0, 13, 4],
    [16, 10, 32, 12],
    [32, 12, 16, 10],
    [0, 0, 16, 4],
    [3, 12, 5, 16],
    [11, 12, 13, 16],
    [32, 9, 16, 7],
    [16, 7, 32, 9],
    [0, 12, 16, 16],
];

/// Panel rectangles for the unpowered front and each of the four powered types.
const NATIVE_FRONTS: [[u16; 4]; 5] = [
    [0, 4, 16, 12],
    [16, 24, 32, 32],
    [16, 16, 32, 24],
    [0, 24, 16, 32],
    [0, 16, 16, 24],
];

/// Model bounds in 1/256 block per emitted quad in the south pose, with the panel last.
const NATIVE_BOUNDS: [([i16; 3], [i16; 3]); 16] = [
    ([0, 0, 0], [0, 256, 48]),
    ([256, 0, 0], [256, 256, 48]),
    ([0, 0, 0], [256, 0, 48]),
    ([0, 256, 0], [256, 256, 48]),
    ([0, 0, 0], [256, 256, 0]),
    ([0, 192, 48], [0, 256, 80]),
    ([256, 192, 48], [256, 256, 80]),
    ([0, 192, 48], [256, 192, 80]),
    ([0, 256, 48], [256, 256, 80]),
    ([0, 192, 80], [256, 256, 80]),
    ([0, 0, 48], [0, 64, 80]),
    ([256, 0, 48], [256, 64, 80]),
    ([0, 0, 48], [256, 0, 80]),
    ([0, 64, 48], [256, 64, 80]),
    ([0, 0, 80], [256, 64, 80]),
    ([0, 64, 48], [256, 192, 48]),
];

/// A distinct opaque color for each texel of the native sheet.
fn native_texel(x: u32, y: u32) -> [u8; 4] {
    [x as u8 * 7, y as u8 * 7, 40, 255]
}

/// Compiles shelves whose `terrain_texture.json` and flipbook catalog come from the caller.
fn compile(
    blocks: &str,
    terrain: &str,
    flipbooks: &str,
    images: &[(&str, u32, u32)],
) -> (CompiledAssets, MaterialKeys, Vec<RegistryRecord>) {
    let (records, lights, protocol) = active_records(&[SHELF]);
    let directory = tempfile::tempdir().unwrap();
    write(directory.path().join("blocks.json"), blocks);
    write(
        directory.path().join("textures/terrain_texture.json"),
        terrain,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        flipbooks,
    );
    for &(path, width, height) in images {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                // Rows repeat, so a taller strip's frames each hold the native sheet.
                pixels.extend_from_slice(&native_texel(
                    x * NATIVE_SHEET / width,
                    (y * NATIVE_SHEET / width) % NATIVE_SHEET,
                ));
            }
        }
        write_png(directory.path().join(path), width, height, &pixels);
    }
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, keys) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, keys, records)
}

/// Compiles shelves that use one static sheet of `side` texels for every face.
fn compile_sheet(side: u32) -> (CompiledAssets, MaterialKeys, Vec<RegistryRecord>) {
    compile(
        r#"{"poplar_shelf":{"textures":"fixture_shelf"}}"#,
        r#"{"texture_data":{"fixture_shelf":{"textures":"textures/fixture/shelf"}}}"#,
        "[]",
        &[("textures/fixture/shelf.png", side, side)],
    )
}

/// Resolves the shelf records to their south-pose rotation count and front index.
fn poses(records: &[RegistryRecord]) -> impl Iterator<Item = (&RegistryRecord, usize, usize)> {
    records
        .iter()
        .filter(|record| record.name.as_ref() == SHELF)
        .map(|record| {
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            let direction = ["south", "west", "north", "east"]
                .iter()
                .position(|name| {
                    state["minecraft:cardinal_direction"]["value"].as_str() == Some(name)
                })
                .unwrap();
            let front = if state["powered_bit"]["value"].as_u64() == Some(0) {
                0
            } else {
                state["powered_shelf_type"]["value"].as_u64().unwrap() as usize + 1
            };
            (record, direction, front)
        })
}

fn template_quads(compiled: &CompiledAssets, template: u32) -> &[ModelQuad] {
    let template = compiled.model_templates[template as usize];
    &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize]
}

fn region_for(quad: usize, front: usize) -> [u16; 4] {
    NATIVE_REGIONS
        .get(quad)
        .copied()
        .unwrap_or(NATIVE_FRONTS[front])
}

/// Reads a layer's base texel, as a nearest-filtered draw at full magnification would.
fn layer_texel(compiled: &CompiledAssets, material: u32, x: u32, y: u32) -> [u8; 4] {
    let texture = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    let start = (texture.layer() * mip.size * mip.size + y * mip.size + x) as usize * 4;
    mip.rgba8[start..start + 4].try_into().unwrap()
}

/// Asserts that each template quad reads the exact native sheet texels across its UV rectangle.
fn assert_quads_sample_native_texels(compiled: &CompiledAssets, records: &[RegistryRecord]) {
    for (record, _, front) in poses(records) {
        let visual = &compiled.visuals[record.sequential_id as usize];
        let quads = template_quads(compiled, visual.model_template);
        assert_eq!(quads.len(), NATIVE_BOUNDS.len());
        for (index, quad) in quads.iter().enumerate() {
            let [u0, v0, u1, v1] = region_for(index, front).map(u32::from);
            let [left, right, top, bottom] = [u0.min(u1), u0.max(u1), v0.min(v1), v0.max(v1)];
            let bound = |axis: usize| {
                let values = quad.uvs.map(|uv| u32::from(uv[axis]));
                [*values.iter().min().unwrap(), *values.iter().max().unwrap()]
            };
            let ([u_min, u_max], [v_min, v_max]) = (bound(0), bound(1));
            for y in top..bottom {
                for x in left..right {
                    let u = u_min + (u_max - u_min) * (2 * (x - left) + 1) / (2 * (right - left));
                    let v = v_min + (v_max - v_min) * (2 * (y - top) + 1) / (2 * (bottom - top));
                    assert_eq!(
                        layer_texel(
                            compiled,
                            quad.material,
                            u * TILE_SIZE / 4096,
                            v * TILE_SIZE / 4096
                        ),
                        native_texel(x, y),
                        "quad {index} of {} at sheet texel ({x}, {y})",
                        record.canonical_state,
                    );
                }
            }
        }
    }
}

#[test]
fn shelf_quads_read_every_native_sheet_texel_at_full_resolution() {
    let (compiled, _, records) = compile_sheet(NATIVE_SHEET);
    assert_quads_sample_native_texels(&compiled, &records);
}

#[test]
fn higher_resolution_sheets_normalize_each_quadrant_to_a_tile() {
    let (compiled, _, records) = compile_sheet(NATIVE_SHEET * 2);
    assert_quads_sample_native_texels(&compiled, &records);
}

#[test]
fn world_and_whole_sheet_templates_match_the_native_table_in_every_pose() {
    let (compiled, _, records) = compile_sheet(NATIVE_SHEET);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    assert_eq!(poses(&records).count(), 32);
    for (record, direction, front) in poses(&records) {
        let visual = &compiled.visuals[record.sequential_id as usize];
        let world = visual.model_template;
        let sheet = assets::whole_sheet_model_template(&compiled.model_templates, world);
        assert_eq!(
            sheet,
            world + 1,
            "the whole-sheet pair follows its world template"
        );
        assert_eq!(
            compiled.model_templates[world as usize].flags,
            MODEL_TEMPLATE_FLAG_SHELF | MODEL_TEMPLATE_FLAG_SHEET_TILES,
        );
        assert_eq!(
            compiled.model_templates[sheet as usize].flags,
            MODEL_TEMPLATE_FLAG_SHELF
        );
        assert_eq!(
            runtime
                .resolve(assets::NetworkIdMode::Sequential, record.sequential_id)
                .model_template(),
            Some(world),
        );
        let (world, sheet) = (
            template_quads(&compiled, world),
            template_quads(&compiled, sheet),
        );
        assert_eq!(world.len(), sheet.len());
        for (index, (world, sheet)) in world.iter().zip(sheet).enumerate() {
            assert_eq!(world.positions, sheet.positions);
            assert_eq!(world.flags, sheet.flags);
            assert!(visual.faces.contains(&sheet.material));
            let mut bounds = [[i16::MAX; 3], [i16::MIN; 3]];
            for position in sheet.positions {
                for axis in 0..3 {
                    bounds[0][axis] = bounds[0][axis].min(position[axis]);
                    bounds[1][axis] = bounds[1][axis].max(position[axis]);
                }
            }
            let (mut min, mut max) = NATIVE_BOUNDS[index];
            for _ in 0..direction {
                (min, max) = (
                    [256 - max[2], min[1], min[0]],
                    [256 - min[2], max[1], max[0]],
                );
            }
            assert_eq!(
                bounds,
                [min, max],
                "quad {index} of {}",
                record.canonical_state
            );
            let [u0, v0, u1, v1] = region_for(index, front);
            for (axis, [low, high]) in [[u0.min(u1), u0.max(u1)], [v0.min(v1), v0.max(v1)]]
                .into_iter()
                .enumerate()
            {
                let values = sheet.uvs.map(|uv| uv[axis]);
                // Native insets each sampled edge by a quarter percent of the sheet.
                assert_eq!(values.iter().min(), Some(&(low * 128 + 10)));
                assert_eq!(values.iter().max(), Some(&(high * 128 - 10)));
                // The tile corner order keeps the sheet's, so mirrored faces stay mirrored.
                let quadrant = u16::from(low * 128 >= 2048) * 2048;
                for (world, sheet) in world.uvs.iter().zip(&sheet.uvs) {
                    assert_eq!(world[axis], (sheet[axis] - quadrant) * 2);
                }
            }
        }
    }
}

#[test]
fn sheet_keys_and_cutout_stay_on_the_whole_sheet_material() {
    let (compiled, keys, records) = compile_sheet(NATIVE_SHEET);
    let record = records
        .iter()
        .find(|record| record.name.as_ref() == SHELF)
        .unwrap();
    let visual = &compiled.visuals[record.sequential_id as usize];
    let sheet = visual.faces[0];
    assert!(visual.faces.iter().all(|&face| face == sheet));
    assert_eq!(keys.materials("fixture_shelf"), [sheet]);
    for quad in template_quads(&compiled, visual.model_template) {
        assert!(
            !keys
                .keys()
                .any(|key| keys.materials(key).contains(&quad.material))
        );
        assert_ne!(
            compiled.materials[quad.material as usize].flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT,
            0,
            "transparent art must keep its cutout",
        );
    }
}

/// GUI previews draw the same full-resolution regions as the placed block.
#[test]
fn inventory_quads_keep_the_full_resolution_regions() {
    let (compiled, _, records) = compile_sheet(NATIVE_SHEET);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let record = records
        .iter()
        .find(|record| record.name.as_ref() == SHELF)
        .unwrap();
    let visual = &compiled.visuals[record.sequential_id as usize];
    let tiled = template_quads(&compiled, visual.model_template);
    let gui = assets::gui_item::block_gui_quads(
        visual.kind,
        visual.faces,
        Some(visual.model_template),
        runtime.model_templates(),
        runtime.model_quads(),
    )
    .unwrap();
    assert!(!gui.is_empty());
    assert!(
        gui.iter()
            .all(|quad| tiled.iter().any(|tile| tile.material == quad.material))
    );
}

fn assert_only_whole_sheet_templates(compiled: &CompiledAssets, records: &[RegistryRecord]) {
    assert!(
        compiled
            .model_templates
            .iter()
            .all(|template| template.flags & MODEL_TEMPLATE_FLAG_SHEET_TILES == 0),
    );
    for (record, ..) in poses(records) {
        let visual = &compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model);
        assert_ne!(
            compiled.model_templates[visual.model_template as usize].flags
                & MODEL_TEMPLATE_FLAG_SHELF,
            0,
        );
    }
}

#[test]
fn tile_sized_sheets_add_no_layers_or_templates() {
    let (compiled, _, records) = compile_sheet(TILE_SIZE);
    assert_only_whole_sheet_templates(&compiled, &records);
    assert_eq!(compiled.model_templates.len(), 20);
}

#[test]
fn animated_sheets_keep_their_whole_sheet_timeline() {
    let (compiled, _, records) = compile(
        r#"{"poplar_shelf":{"textures":"fixture_shelf"}}"#,
        r#"{"texture_data":{"fixture_shelf":{"textures":"textures/fixture/shelf"}}}"#,
        r#"[{"flipbook_texture":"textures/fixture/shelf","atlas_tile":"fixture_shelf","ticks_per_frame":2}]"#,
        &[("textures/fixture/shelf.png", NATIVE_SHEET, NATIVE_SHEET * 2)],
    );
    assert_only_whole_sheet_templates(&compiled, &records);
    let record = records
        .iter()
        .find(|record| record.name.as_ref() == SHELF)
        .unwrap();
    let sheet = compiled.visuals[record.sequential_id as usize].faces[0];
    assert_ne!(
        compiled.materials[sheet as usize].animation,
        assets::NO_ANIMATION
    );
}

#[test]
fn per_face_sheets_keep_their_authored_face_routing() {
    let (compiled, _, records) = compile(
        r#"{"poplar_shelf":{"textures":{"up":"fixture_top","down":"fixture_top","side":"fixture_side"}}}"#,
        r#"{"texture_data":{
            "fixture_top":{"textures":"textures/fixture/top"},
            "fixture_side":{"textures":"textures/fixture/side"}
        }}"#,
        "[]",
        &[
            ("textures/fixture/top.png", NATIVE_SHEET, NATIVE_SHEET),
            ("textures/fixture/side.png", NATIVE_SHEET, NATIVE_SHEET),
        ],
    );
    assert_only_whole_sheet_templates(&compiled, &records);
    let record = records
        .iter()
        .find(|record| record.name.as_ref() == SHELF)
        .unwrap();
    let faces = compiled.visuals[record.sequential_id as usize].faces;
    assert_ne!(
        faces[BlockFace::Up as usize],
        faces[BlockFace::North as usize]
    );
}
