use super::*;
use crate::{
    hud::{LEGACY_ICON_RECTS, Rect},
    texture::{Budget, encode},
};
use image::{Rgba, RgbaImage};
use std::collections::BTreeMap;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];

/// A `width` x `height` PNG filled with `color`.
fn solid(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    encode(&RgbaImage::from_pixel(width, height, Rgba(color)))
}

/// A transparent `size`-pixel atlas with each region, given in 256-pixel coordinates and scaled
/// with the atlas, filled with its colour.
fn marked_atlas(size: u32, regions: &[(Rect, [u8; 4])]) -> Vec<u8> {
    let scale = size / 256;
    let mut image = RgbaImage::new(size, size);
    for (region, color) in regions {
        for y in region.y * scale..(region.y + region.h) * scale {
            for x in region.x * scale..(region.x + region.w) * scale {
                image.put_pixel(x, y, Rgba(*color));
            }
        }
    }
    encode(&image)
}

/// The legacy icon region named `name`.
fn icon(name: &str) -> Rect {
    LEGACY_ICON_RECTS
        .iter()
        .find(|(icon, _)| *icon == name)
        .unwrap()
        .1
}

/// The runtime texture source limit the tests translate under.
const TEXTURE_LIMIT: u64 = 4 * 1024 * 1024;

/// Translates in-memory pack files.
fn run(files: &BTreeMap<String, Vec<u8>>, name: &str) -> Result<Translation, TranslateError> {
    let paths: Vec<String> = files.keys().cloned().collect();
    translate(&paths, |path| files.get(path).cloned(), name, TEXTURE_LIMIT)
}

/// Translated files keyed by path.
fn output(translation: &Translation) -> BTreeMap<&str, &[u8]> {
    translation
        .files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect()
}

/// The size of a translated texture and its pixel at (`x`, `y`).
fn pixel(out: &BTreeMap<&str, &[u8]>, path: &str, x: u32, y: u32) -> ((u32, u32), [u8; 4]) {
    let bytes = out.get(path).unwrap_or_else(|| panic!("{path} missing"));
    let format = if path.ends_with(".tga") {
        image::ImageFormat::Tga
    } else {
        image::ImageFormat::Png
    };
    let image = image::load_from_memory_with_format(bytes, format)
        .unwrap()
        .into_rgba8();
    (image.dimensions(), image.get_pixel(x, y).0)
}

/// Builds a file map from string paths.
fn pack(files: Vec<(&str, Vec<u8>)>) -> BTreeMap<String, Vec<u8>> {
    files
        .into_iter()
        .map(|(path, bytes)| (path.to_owned(), bytes))
        .collect()
}

#[test]
fn a_legacy_pack_translates_textures_hud_potions_and_sky() {
    let icons = marked_atlas(
        512,
        &[
            (icon("crosshair"), WHITE),
            (icon("heart/full"), RED),
            (icon("food_full"), YELLOW),
        ],
    );
    let region = |x, y, w, h| Rect { x, y, w, h };
    let widgets = marked_atlas(
        256,
        &[
            (region(0, 0, 1, 22), RED),
            (region(1, 0, 20, 22), GREEN),
            (region(161, 0, 20, 22), BLUE),
            (region(0, 22, 24, 24), WHITE),
        ],
    );
    let textures = "assets/minecraft/textures/";
    let files = pack(vec![
        (
            "pack.mcmeta",
            "{\"pack\":{\"pack_format\":1,\"description\":\"\u{a7}6Legacy \u{a7}rPvP\"}}".into(),
        ),
        ("pack.png", solid(16, 16, BLUE)),
        (
            &format!("{textures}blocks/planks_oak.png"),
            solid(16, 16, GREEN),
        ),
        (
            &format!("{textures}blocks/destroy_stage_3.png"),
            solid(16, 16, GREEN),
        ),
        (
            &format!("{textures}items/fish_cod_raw.png"),
            solid(16, 16, BLUE),
        ),
        (
            &format!("{textures}items/potion_overlay.png"),
            solid(16, 16, WHITE),
        ),
        (
            &format!("{textures}items/potion_bottle_splash.png"),
            encode(&RgbaImage::new(16, 16)),
        ),
        (
            &format!("{textures}models/armor/diamond_layer_1.png"),
            solid(64, 32, BLUE),
        ),
        (
            &format!("{textures}models/armor/chainmail_layer_2.png"),
            solid(64, 32, RED),
        ),
        (&format!("{textures}gui/icons.png"), icons),
        (&format!("{textures}gui/widgets.png"), widgets),
        (
            &format!("{textures}particle/particles.png"),
            solid(128, 128, RED),
        ),
        (
            "assets/minecraft/mcpatcher/sky/world0/sky1.properties",
            b"source=./sky1.png\n".to_vec(),
        ),
        (
            "assets/minecraft/mcpatcher/sky/world0/sky1.png",
            solid(48, 32, BLUE),
        ),
        ("assets/minecraft/sounds/ignored.ogg", vec![1, 2, 3]),
    ]);
    let translation = run(&files, "Legacy PvP").unwrap();
    assert_eq!(translation.layout, Layout::Legacy);
    let out = output(&translation);

    let manifest: serde_json::Value = serde_json::from_slice(out["manifest.json"]).unwrap();
    assert_eq!(manifest["format_version"], 2);
    assert_eq!(manifest["header"]["name"], "Legacy PvP");
    assert_eq!(manifest["header"]["description"], "Legacy PvP");
    assert_eq!(
        manifest["header"]["min_engine_version"],
        serde_json::json!(MIN_ENGINE_VERSION)
    );
    assert_eq!(manifest["modules"][0]["type"], "resources");
    for path in [
        "pack_icon.png",
        "textures/blocks/planks_oak.png",
        "textures/environment/destroy_stage_3.png",
        "textures/items/fish_raw.png",
        "textures/models/armor/diamond_1.png",
        "textures/models/armor/chain_2.png",
        "textures/gui/icons.png",
        "textures/particle/particles.png",
        "textures/environment/overworld_cubemap/cubemap_0.png",
        "textures/environment/overworld_cubemap/cubemap_5.png",
    ] {
        assert!(out.contains_key(path), "{path} missing");
    }
    assert!(!out.keys().any(|path| path.contains("sounds")));

    let (_, color) = pixel(&out, "textures/items/potion_bottle_splash_heal.png", 8, 8);
    assert_eq!(
        color,
        [245, 35, 34, 255],
        "white overlay tinted with the heal colour"
    );
    assert!(!out.contains_key("textures/items/potion_bottle_heal.png"));

    assert_eq!(pixel(&out, "textures/ui/heart.png", 0, 0), ((18, 18), RED));
    assert_eq!(pixel(&out, "textures/ui/hunger_full.png", 5, 5).1, YELLOW);
    assert!(
        !out.contains_key("textures/ui/freeze_heart.png"),
        "empty atlas region exported"
    );

    assert_eq!(
        pixel(&out, "textures/ui/hotbar_start_cap.png", 0, 0),
        ((1, 22), RED)
    );
    assert_eq!(
        pixel(&out, "textures/ui/hotbar_0.png", 0, 0),
        ((20, 22), GREEN)
    );
    assert_eq!(pixel(&out, "textures/ui/hotbar_8.png", 19, 21).1, BLUE);
    assert_eq!(
        pixel(&out, "textures/ui/selected_hotbar_slot.png", 0, 0).0,
        (24, 24)
    );
}

#[test]
fn a_current_pack_translates_flattened_names_strips_sprites_and_equipment() {
    let sprites = "assets/minecraft/textures/gui/sprites/hud/";
    let selection = RgbaImage::from_fn(24, 23, |_, y| match y {
        0 => Rgba(WHITE),
        22 => Rgba(RED),
        _ => Rgba([0; 4]),
    });
    let strip = encode(&RgbaImage::from_fn(16, 64, |_, y| {
        Rgba(if y < 16 { RED } else { BLUE })
    }));
    let blocks = "assets/minecraft/textures/block/";
    let files = pack(vec![
        (
            "pack.mcmeta",
            br#"{"pack":{"description":{"text":"Current ","extra":[{"text":"Pack"}]},"min_format":[97,1]}}"#
                .to_vec(),
        ),
        (&format!("{blocks}oak_planks.png"), solid(16, 16, GREEN)),
        (&format!("{blocks}fire_0.png"), strip.clone()),
        (&format!("{blocks}stone.png"), strip),
        ("assets/minecraft/textures/item/golden_apple.png", solid(16, 16, YELLOW)),
        (&format!("{sprites}heart/full.png"), solid(9, 9, RED)),
        (&format!("{sprites}crosshair.png"), solid(15, 15, WHITE)),
        (&format!("{sprites}hotbar.png"), solid(182, 22, GREEN)),
        (&format!("{sprites}hotbar_selection.png"), encode(&selection)),
        (
            "assets/minecraft/textures/entity/equipment/humanoid_leggings/iron.png",
            solid(64, 32, WHITE),
        ),
        (
            "assets/minecraft/textures/entity/equipment/humanoid/turtle_scute.png",
            solid(64, 32, GREEN),
        ),
    ]);
    let translation = run(&files, "Current").unwrap();
    assert_eq!(translation.layout, Layout::Flattened);
    let out = output(&translation);
    let manifest: serde_json::Value = serde_json::from_slice(out["manifest.json"]).unwrap();
    assert_eq!(manifest["header"]["description"], "Current Pack");

    assert!(out.contains_key("textures/blocks/planks_oak.png"));
    assert!(out.contains_key("textures/items/apple_golden.png"));
    assert_eq!(
        pixel(&out, "textures/blocks/fire_0.png", 0, 63),
        ((16, 64), BLUE)
    );
    assert_eq!(
        pixel(&out, "textures/blocks/stone.png", 0, 0),
        ((16, 16), RED)
    );
    assert_eq!(pixel(&out, "textures/ui/heart.png", 0, 0), ((9, 9), RED));
    assert_eq!(
        pixel(&out, "textures/ui/hotbar_4.png", 0, 0),
        ((20, 22), GREEN)
    );
    let (size, top) = pixel(&out, "textures/ui/selected_hotbar_slot.png", 0, 23);
    assert_eq!(
        (size, top),
        ((24, 24), WHITE),
        "padded with the top border row"
    );
    assert_eq!(
        pixel(&out, "textures/gui/icons.png", 0, 0),
        ((256, 256), WHITE)
    );
    assert!(out.contains_key("textures/models/armor/iron_2.png"));
    assert!(out.contains_key("textures/models/armor/turtle_1.png"));
}

/// A vertical strip of `frames` square frames, each filled with a colour derived from its index.
fn strip(side: u32, frames: u32) -> Vec<u8> {
    encode(&RgbaImage::from_fn(side, side * frames, |_, y| {
        Rgba([(y / side) as u8, 0, 0, 255])
    }))
}

#[test]
fn pre_1_9_clock_and_compass_strips_become_the_bedrock_atlases() {
    let items = "assets/minecraft/textures/items/";
    let files = pack(vec![
        (
            "pack.mcmeta",
            br#"{"pack":{"pack_format":1,"description":"1.7"}}"#.to_vec(),
        ),
        (&format!("{items}clock.png"), strip(16, 64)),
        (&format!("{items}compass.png"), strip(16, 32)),
        (&format!("{items}bed.png"), solid(16, 16, RED)),
    ]);
    let translation = run(&files, "Legacy").unwrap();
    let out = output(&translation);
    assert_eq!(
        pixel(&out, "textures/items/watch_atlas.png", 0, 63 * 16),
        ((16, 1024), [63, 0, 0, 255])
    );
    assert_eq!(
        pixel(&out, "textures/items/compass_atlas.png", 0, 31 * 16),
        ((16, 512), [31, 0, 0, 255])
    );
    assert!(!out.contains_key("textures/items/clock.png"));
    assert!(!out.contains_key("textures/items/compass.png"));
    assert!(out.contains_key("textures/items/bed_red.png"));
}

#[test]
fn strips_with_another_frame_count_are_not_used_as_atlases() {
    let files = pack(vec![(
        "assets/minecraft/textures/items/clock.png",
        strip(16, 8),
    )]);
    let out_files = run(&files, "Legacy").unwrap();
    let out = output(&out_files);
    assert!(!out.contains_key("textures/items/watch_atlas.png"));
    let flattened = pack(vec![(
        "assets/minecraft/textures/item/clock.png",
        strip(16, 64),
    )]);
    let translation = run(&flattened, "Current").unwrap();
    assert!(!output(&translation).contains_key("textures/items/watch_atlas.png"));
}

#[test]
fn textures_in_subfolders_never_replace_vanilla_textures() {
    let files = pack(vec![
        (
            "assets/minecraft/textures/block/stone.png",
            solid(16, 16, RED),
        ),
        (
            "assets/minecraft/textures/block/variants/stone.png",
            solid(16, 16, BLUE),
        ),
        (
            "assets/minecraft/textures/item/custom/apple.png",
            solid(16, 16, BLUE),
        ),
    ]);
    let translation = run(&files, "Pack").unwrap();
    let out = output(&translation);
    assert_eq!(pixel(&out, "textures/blocks/stone.png", 0, 0).1, RED);
    assert!(!out.contains_key("textures/items/apple.png"));
}

#[test]
fn partially_transparent_pixels_keep_their_alpha() {
    let translucent = encode(&RgbaImage::from_pixel(16, 16, Rgba([10, 20, 30, 254])));
    let files = pack(vec![
        (
            "assets/minecraft/textures/item/apple.png",
            translucent.clone(),
        ),
        (
            "assets/minecraft/textures/gui/icons.png",
            encode(&RgbaImage::from_pixel(256, 256, Rgba([1, 2, 3, 200]))),
        ),
    ]);
    let translation = run(&files, "Pack").unwrap();
    let out = output(&translation);
    assert_eq!(out["textures/items/apple.png"], translucent.as_slice());
    assert_eq!(pixel(&out, "textures/ui/heart.png", 0, 0).1, [1, 2, 3, 200]);
}

#[test]
fn hostile_hud_sprites_cannot_request_huge_canvases() {
    let sprites = "assets/minecraft/textures/gui/sprites/hud/";
    let files = pack(vec![
        (&format!("{sprites}crosshair.png"), solid(16_380, 1, WHITE)),
        (
            &format!("{sprites}hotbar_selection.png"),
            solid(16_380, 1, WHITE),
        ),
        (
            "assets/minecraft/textures/block/stone.png",
            solid(16, 16, RED),
        ),
    ]);
    let translation = run(&files, "Pack").unwrap();
    let out = output(&translation);
    assert!(!out.contains_key("textures/gui/icons.png"));
    assert!(!out.contains_key("textures/ui/selected_hotbar_slot.png"));
    assert!(out.contains_key("textures/blocks/stone.png"));
}

#[test]
fn textures_vanilla_ships_as_tga_are_written_as_tga() {
    let files = pack(vec![
        (
            "assets/minecraft/textures/block/oak_leaves.png",
            solid(16, 16, GREEN),
        ),
        (
            "assets/minecraft/textures/item/leather_helmet.png",
            solid(16, 16, RED),
        ),
        (
            "assets/minecraft/textures/block/stone.png",
            solid(16, 16, RED),
        ),
    ]);
    let translation = run(&files, "Pack").unwrap();
    let out = output(&translation);
    assert_eq!(
        pixel(&out, "textures/blocks/leaves_oak.tga", 0, 0),
        ((16, 16), GREEN)
    );
    assert!(!out.contains_key("textures/blocks/leaves_oak.png"));
    assert!(
        !out.keys().any(|path| path.contains("leather_helmet")),
        "dye masks are left alone"
    );
}

#[test]
fn grass_sides_become_tint_masks() {
    let overlay = encode(&RgbaImage::from_fn(16, 16, |_, y| {
        Rgba(if y < 4 { [150, 150, 150, 255] } else { [0; 4] })
    }));
    let blocks = "assets/minecraft/textures/block/";
    let files = pack(vec![
        (
            &format!("{blocks}grass_block_side.png"),
            solid(16, 16, [120, 80, 50, 255]),
        ),
        (
            &format!("{blocks}grass_block_side_overlay.png"),
            overlay.clone(),
        ),
        (
            &format!("{blocks}grass_block_snow.png"),
            solid(16, 16, WHITE),
        ),
    ]);
    let out_files = run(&files, "Current").unwrap();
    let out = output(&out_files);
    assert_eq!(
        pixel(&out, "textures/blocks/grass_side.tga", 0, 0).1,
        [150, 150, 150, 255]
    );
    assert_eq!(
        pixel(&out, "textures/blocks/grass_side.tga", 0, 8).1,
        [120, 80, 50, 0]
    );
    assert_eq!(
        pixel(&out, "textures/blocks/grass_side_snowed.tga", 3, 3).1,
        [255, 255, 255, 0]
    );
    assert!(out.contains_key("textures/blocks/grass_side_carried.png"));

    let legacy_side_only = pack(vec![
        (
            "assets/minecraft/textures/blocks/grass_side.png",
            solid(16, 16, [120, 80, 50, 255]),
        ),
        (
            "assets/minecraft/textures/blocks/stone.png",
            solid(16, 16, RED),
        ),
    ]);
    let translation = run(&legacy_side_only, "Legacy").unwrap();
    let out = output(&translation);
    assert!(
        !out.keys()
            .any(|path| path.starts_with("textures/blocks/grass_side."))
    );
}

#[test]
fn default_potion_bottles_are_filled_with_water() {
    let items = "assets/minecraft/textures/item/";
    let files = pack(vec![
        (&format!("{items}potion_overlay.png"), solid(16, 16, WHITE)),
        (
            &format!("{items}potion.png"),
            encode(&RgbaImage::new(16, 16)),
        ),
    ]);
    let translation = run(&files, "Current").unwrap();
    let out = output(&translation);
    assert_eq!(
        pixel(&out, "textures/items/potion_bottle_drinkable.png", 8, 8).1,
        [56, 92, 196, 255]
    );
    assert_eq!(
        pixel(&out, "textures/items/potion_bottle_heal.png", 8, 8).1,
        [245, 35, 34, 255]
    );
}

#[test]
fn translations_beyond_the_pixel_budget_fail() {
    let files = pack(vec![
        (
            "assets/minecraft/textures/item/potion_overlay.png",
            solid(64, 64, WHITE),
        ),
        (
            "assets/minecraft/textures/item/potion.png",
            solid(64, 64, RED),
        ),
    ]);
    let paths: Vec<String> = files.keys().cloned().collect();
    let read = |path: &str| files.get(path).cloned();
    let small = Budget::with_pixels(64 * 64 * 10);
    assert_eq!(
        translate::translate_within(&paths, read, "Pack", TEXTURE_LIMIT, &small).unwrap_err(),
        TranslateError::TooLarge
    );
    assert!(run(&files, "Pack").is_ok());
}

#[test]
fn modern_crosshairs_replace_the_legacy_atlas_crosshair() {
    let textures = "assets/minecraft/textures/";
    let icons = marked_atlas(
        256,
        &[(icon("crosshair"), WHITE), (icon("heart/full"), RED)],
    );
    let files = pack(vec![
        (&format!("{textures}gui/icons.png"), icons),
        (
            &format!("{textures}gui/sprites/hud/crosshair.png"),
            solid(15, 15, BLUE),
        ),
    ]);
    let translation = run(&files, "Pack").unwrap();
    let out = output(&translation);
    assert_eq!(
        pixel(&out, "textures/gui/icons.png", 0, 0),
        ((256, 256), BLUE)
    );
    assert_eq!(
        pixel(&out, "textures/gui/icons.png", 52, 0).1,
        RED,
        "the rest of the atlas stays"
    );
}

#[test]
fn bottles_without_an_overlay_are_not_published_as_filled_bottles() {
    let files = pack(vec![
        (
            "assets/minecraft/textures/item/potion.png",
            encode(&RgbaImage::new(16, 16)),
        ),
        (
            "assets/minecraft/textures/block/stone.png",
            solid(16, 16, RED),
        ),
    ]);
    let translation = run(&files, "Pack").unwrap();
    assert!(!output(&translation).contains_key("textures/items/potion_bottle_drinkable.png"));
}

#[test]
fn generated_textures_over_the_runtime_limit_are_left_out() {
    let files = pack(vec![
        (
            "assets/minecraft/textures/block/oak_leaves.png",
            solid(16, 16, GREEN),
        ),
        (
            "assets/minecraft/textures/block/stone.png",
            solid(16, 16, RED),
        ),
    ]);
    let paths: Vec<String> = files.keys().cloned().collect();
    let translation = translate(&paths, |path| files.get(path).cloned(), "Pack", 8).unwrap();
    let out = output(&translation);
    assert!(!out.contains_key("textures/blocks/leaves_oak.tga"));
    assert!(
        out.contains_key("textures/blocks/stone.png"),
        "copied sources are kept"
    );
    assert_eq!(translation.skipped, 1);

    let only_oversized = pack(vec![(
        "assets/minecraft/textures/block/oak_leaves.png",
        solid(16, 16, GREEN),
    )]);
    let paths: Vec<String> = only_oversized.keys().cloned().collect();
    assert_eq!(
        translate(&paths, |path| only_oversized.get(path).cloned(), "Pack", 8).unwrap_err(),
        TranslateError::NothingToTranslate
    );
}

#[test]
fn experience_bars_reach_the_hud_textures_as_plain_images() {
    let sprites = "assets/minecraft/textures/gui/sprites/hud/";
    let files = pack(vec![
        (
            &format!("{sprites}experience_bar_background.png"),
            solid(182, 5, GREEN),
        ),
        (
            &format!("{sprites}experience_bar_progress.png"),
            solid(182, 5, YELLOW),
        ),
    ]);
    let translation = run(&files, "Pack").unwrap();
    let out = output(&translation);
    assert_eq!(
        pixel(&out, "textures/ui/experiencebarempty.png", 181, 4),
        ((182, 5), GREEN)
    );
    assert_eq!(
        pixel(&out, "textures/ui/experiencebarfull.png", 0, 0),
        ((182, 5), YELLOW)
    );
    for sidecar in [
        "textures/ui/experiencebarempty.json",
        "textures/ui/experiencebarfull.json",
    ] {
        let value: serde_json::Value = serde_json::from_slice(out[sidecar]).unwrap();
        assert_eq!(value, serde_json::json!({"base_size": [182, 5]}));
    }
    assert_eq!(pixel(&out, "textures/ui/experiencenub.png", 0, 0).1, [0; 4]);
}

#[test]
fn animated_potion_layers_compose_their_first_frame() {
    let items = "assets/minecraft/textures/item/";
    let overlay = encode(&RgbaImage::from_fn(16, 32, |_, y| {
        Rgba(if y < 16 { WHITE } else { BLUE })
    }));
    let files = pack(vec![
        (&format!("{items}potion_overlay.png"), overlay),
        (
            &format!("{items}potion.png"),
            encode(&RgbaImage::new(16, 16)),
        ),
    ]);
    let translation = run(&files, "Pack").unwrap();
    let out = output(&translation);
    assert_eq!(
        pixel(&out, "textures/items/potion_bottle_heal.png", 8, 8),
        ((16, 16), [245, 35, 34, 255])
    );
}

#[test]
fn translating_the_same_content_keeps_the_pack_identity() {
    let files = pack(vec![(
        "assets/minecraft/textures/block/stone.png",
        solid(16, 16, RED),
    )]);
    let identity = |files: &BTreeMap<String, Vec<u8>>| {
        let translation = run(files, "Pack").unwrap();
        let out = output(&translation);
        let manifest: serde_json::Value = serde_json::from_slice(out["manifest.json"]).unwrap();
        manifest["header"]["uuid"].as_str().unwrap().to_owned()
    };
    let first = identity(&files);
    assert_eq!(first, identity(&files));
    let changed = pack(vec![(
        "assets/minecraft/textures/block/stone.png",
        solid(16, 16, BLUE),
    )]);
    assert_ne!(first, identity(&changed));
}

#[test]
fn unreadable_and_invalid_files_are_skipped() {
    let files = pack(vec![
        (
            "assets/minecraft/textures/block/stone.png",
            solid(16, 16, RED),
        ),
        (
            "assets/minecraft/textures/block/dirt.png",
            b"not a png".to_vec(),
        ),
        (
            "assets/minecraft/textures/item/apple.png",
            solid(16, 16, RED),
        ),
    ]);
    let paths: Vec<String> = files.keys().cloned().collect();
    let translation = translate(
        &paths,
        |path| (!path.ends_with("apple.png")).then(|| files[path].clone()),
        "Pack",
        TEXTURE_LIMIT,
    )
    .unwrap();
    assert_eq!(translation.skipped, 2);
    let out = output(&translation);
    assert!(out.contains_key("textures/blocks/stone.png"));
    assert!(!out.contains_key("textures/blocks/dirt.png"));
}

#[test]
fn a_pack_without_usable_textures_is_rejected() {
    let files = pack(vec![
        ("pack.mcmeta", br#"{"pack":{"pack_format":15}}"#.to_vec()),
        ("assets/minecraft/sounds.json", b"{}".to_vec()),
    ]);
    assert_eq!(
        run(&files, "Sounds").unwrap_err(),
        TranslateError::NothingToTranslate
    );
}

#[test]
fn roots_are_found_at_the_shallowest_marker() {
    assert_eq!(
        find_root(["pack.mcmeta", "x/pack.mcmeta"]).as_deref(),
        Some("")
    );
    assert_eq!(
        find_root(["Faithful/pack.mcmeta", "Faithful/assets/minecraft/a.png"]).as_deref(),
        Some("Faithful/")
    );
    assert_eq!(
        find_root(["p/assets/minecraft/textures/a.png"]).as_deref(),
        Some("p/")
    );
    assert_eq!(
        find_root([
            "data/minecraft/datapacks/extra/pack.mcmeta",
            "assets/minecraft/textures/block/stone.png",
        ])
        .as_deref(),
        Some(""),
        "a nested data pack never wins"
    );
    assert_eq!(
        find_root(["data/minecraft/datapacks/extra/pack.mcmeta"]).as_deref(),
        Some("data/minecraft/datapacks/extra/")
    );
    assert_eq!(
        find_root(["manifest.json", "textures/a.png", "notpack.mcmeta"]),
        None
    );
}
