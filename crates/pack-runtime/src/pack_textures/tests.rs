use super::*;

#[test]
fn downscaling_weights_colors_by_alpha_coverage() {
    for background in [[0, 0, 0, 0], [0, 255, 0, 0]] {
        for alpha in [64, 255] {
            let mut texture = DecodedTexture {
                width: 2,
                height: 2,
                rgba8: [[240, 120, 60, alpha], background, background, background]
                    .concat()
                    .into(),
            };
            texture.downscale_to_max(1);
            assert_eq!([texture.width, texture.height], [1, 1]);
            assert_eq!(
                texture.rgba8.as_ref(),
                &[240, 120, 60, (f32::from(alpha) / 4.0).round() as u8]
            );
        }
    }
}

#[test]
fn downscaling_fully_transparent_colors_produces_clear_pixels() {
    let mut texture = DecodedTexture {
        width: 4,
        height: 2,
        rgba8: [240, 120, 60, 0].repeat(8).into(),
    };
    texture.downscale_to_max(2);
    assert_eq!([texture.width, texture.height], [2, 1]);
    assert_eq!(texture.rgba8.as_ref(), &[0; 8]);
}

#[test]
fn downscaling_clears_colors_when_alpha_rounds_to_zero() {
    let mut texture = DecodedTexture {
        width: 2,
        height: 2,
        rgba8: [[240, 120, 60, 1], [0; 4], [0; 4], [0; 4]].concat().into(),
    };
    texture.downscale_to_max(1);
    assert_eq!([texture.width, texture.height], [1, 1]);
    assert_eq!(texture.rgba8.as_ref(), &[0; 4]);
}

#[test]
fn pack_json_matches_previous_normalized_bytes() {
    for bytes in [
        br#"{"texture_data":{"a":{"textures":"textures/first"},"a":{"textures":"textures/last"}},"number":1.0000000000000002}"#.as_slice(),
        b"\xef\xbb\xbf{/* catalog */\"texture_data\":{\"caf\xe9\":{\"textures\":\"textures/a\",},},}",
        br#"{"number":18446744073709551616}"#,
        b"{\"texture_data\":{,}}",
        b"{} {}",
        b"1e309",
    ] {
        let before = pack_json::normalize_jsonc(bytes)
            .and_then(|normalized| serde_json::from_slice::<Value>(&normalized).ok());
        assert_eq!(
            parse_pack_json(bytes).map(|value| serde_json::to_vec(&value).unwrap()),
            before.map(|value| serde_json::to_vec(&value).unwrap()),
        );
    }
}

#[test]
fn full_catalogs_keep_later_valid_overrides_after_skipping_new_keys() {
    let mut paths = (0..MAX_CATALOG_ENTRIES)
        .map(|index| (format!("z_{index:05}"), "textures/old".to_owned()))
        .collect::<HashMap<_, _>>();
    let mut grids = paths
        .keys()
        .map(|key| (key.clone(), 1))
        .collect::<HashMap<_, _>>();
    let layer = br#"{"texture_data":{
        "a_new":{"textures":"textures/unadmitted"},
        "z_00000":{"textures":"textures/replacement"},
        "z_00001":{"textures":false},
        "z_00002":{"textures":"textures/gridded","quad":1}
    }}"#;
    merge_texture_catalog(&mut paths, layer);
    merge_terrain_grids(&mut grids, layer);
    assert_eq!(paths.len(), MAX_CATALOG_ENTRIES);
    assert_eq!(grids.len(), MAX_CATALOG_ENTRIES);
    assert!(!paths.contains_key("a_new"));
    assert!(!grids.contains_key("a_new"));
    assert_eq!(paths["z_00000"], "textures/replacement");
    assert_eq!(
        grids["z_00000"], 0,
        "omitted grids reset on a valid override"
    );
    assert_eq!(paths["z_00001"], "textures/old");
    assert_eq!(
        grids["z_00001"], 1,
        "malformed overrides retain the lower grid"
    );
    assert_eq!(paths["z_00002"], "textures/gridded");
    assert_eq!(grids["z_00002"], 1 & assets::TERRAIN_QUAD_SHIFT_MASK as u8);
}

/// Encodes original row and column detail into a synthetic PNG.
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([x as u8, y as u8, (y / width) as u8, 255])
    });
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// Encodes one opaque red pixel as a PNG.
fn red_png() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// One view whose `higher` pack takes precedence over its `lower` pack.
fn layered(lower: &[(&str, &[u8])], higher: &[(&str, &[u8])]) -> LayeredPackView {
    let lower = crate::test_support::stack(lower);
    let higher = crate::test_support::stack(higher);
    LayeredPackView::new(std::sync::Arc::new(
        resource_pack::ValidatedPackStack::compose(&lower, &higher).unwrap(),
    ))
}

#[test]
fn large_strips_and_particle_atlases_retain_every_source_pixel() {
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    for (width, height, strip) in [
        (64, 2048, true),
        (128, 4096, true),
        (64, 8384, true),
        (2048, 2048, false),
    ] {
        let bytes = png(width, height);
        let view =
            LayeredPackView::new(crate::test_support::stack(&[("textures/test.png", &bytes)]));
        let texture =
            decode_pack_texture_with_policy(&view, "textures/test", policy, strip).unwrap();
        assert_eq!([texture.width, texture.height], [width, height]);
        assert_eq!(texture.rgba8.len(), (width * height * 4) as usize);
        let x = width - 1;
        let y = height - 1;
        assert_eq!(
            &texture.rgba8[((y * width + x) * 4) as usize..][..4],
            &[x as u8, y as u8, (y / width) as u8, 255]
        );
        assert!(decode_pack_texture(&view, "textures/test").is_none());
    }
}

#[test]
fn horizontal_strip_decoding_checks_both_axes_before_pixel_allocation() {
    let policy = TexturePolicy::for_device(64);
    for (width, height) in [(8192, 64), (64, 8192)] {
        let bytes = png(width, height);
        let view =
            LayeredPackView::new(crate::test_support::stack(&[("textures/test.png", &bytes)]));
        let texture =
            decode_pack_texture_with_policy(&view, "textures/test", policy, true).unwrap();
        assert_eq!([texture.width, texture.height], [width, height]);
        assert_eq!(texture.rgba8.len(), (width * height * 4) as usize);
        assert_eq!(
            &texture.rgba8[texture.rgba8.len() - 4..],
            &[
                (width - 1) as u8,
                (height - 1) as u8,
                ((height - 1) / width) as u8,
                255
            ]
        );
        assert!(decode_pack_texture_with_policy(&view, "textures/test", policy, false).is_none());
    }
    for (width, height) in [(8193, 64), (8192, 65)] {
        assert!(
            decode_image_with_policy(&png(width, height), ImageFormat::Png, policy, true).is_none()
        );
    }
}

#[test]
fn malformed_over_budget_and_device_oversized_images_are_rejected() {
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    assert!(decode_image_with_policy(b"broken", ImageFormat::Png, policy, true).is_none());
    assert!(decode_image_with_policy(&png(2048, 4096), ImageFormat::Png, policy, true).is_none());
    assert!(decode_image_with_policy(&png(64, 8385), ImageFormat::Png, policy, true).is_none());
    assert!(
        decode_image_with_policy(
            &png(2048, 2048),
            ImageFormat::Png,
            TexturePolicy::for_device(1024),
            false
        )
        .is_none()
    );
}

#[test]
fn resolved_images_keep_the_filename_selected_after_decode_and_texture_set_fallbacks() {
    let mut tga = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 30, 200, 255]))
        .write_to(&mut tga, ImageFormat::Tga)
        .unwrap();
    let png = png(16, 16);
    let view = LayeredPackView::new(crate::test_support::stack(&[
        ("textures/fallback.png", b"invalid image"),
        ("textures/fallback.tga", tga.get_ref()),
        ("textures/shared.png", &png),
        (
            "textures/alias.texture_set.json",
            br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":"shared"}}"#,
        ),
        (
            "textures/solid.texture_set.json",
            br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":[1,2,3,255]}}"#,
        ),
    ]));
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    let fallback =
        resolve_pack_texture_with_policy(&view, "textures/fallback", policy, false).unwrap();
    assert_eq!(
        fallback.image_path,
        image_identity(0, "textures/fallback.tga")
    );
    assert_eq!(&fallback.texture.rgba8[..4], &[20, 30, 200, 255]);
    for path in [
        "textures/shared",
        "./textures/shared.png",
        "textures/shared.PNG",
        "textures/alias",
    ] {
        let image = resolve_pack_texture_with_policy(&view, path, policy, false).unwrap();
        assert_eq!(image.image_path, image_identity(0, "textures/shared.png"));
    }
    let solid = resolve_pack_texture_with_policy(&view, "textures/solid", policy, false).unwrap();
    assert_eq!(
        solid.image_path,
        image_identity(0, "textures/solid.texture_set.json")
    );
    assert_eq!(&solid.texture.rgba8[..], &[1, 2, 3, 255]);
}

#[test]
fn indexed_alias_candidates_preserve_precedence_without_reading_image_payloads() {
    let stack = crate::test_support::stack(&[
        ("textures/shared.png", b"metadata only"),
        ("textures/shared.tga", b"metadata only"),
        (
            "textures/alias.texture_set.json",
            br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":"shared"}}"#,
        ),
    ]);
    let view = LayeredPackView::tracked(stack);
    for path in ["textures/shared", "./textures/shared.PNG", "textures/alias"] {
        assert_eq!(
            indexed_image_candidate(&view, path),
            Some(image_identity(
                0,
                if path.ends_with(".PNG") {
                    "textures/shared.png"
                } else {
                    "textures/shared.tga"
                }
            ))
        );
    }
    assert_eq!(indexed_image_candidate(&view, "textures/uncached"), None);
    let dependencies = view.dependencies().unwrap().snapshot();
    assert!(!dependencies.iter().any(|dependency| matches!(dependency,
        resource_pack::PackDependency::File { path, limit }
            if (path.ends_with(".png") || path.ends_with(".tga"))
                && *limit == resource_pack::MAX_PACK_TEXTURE_BYTES
    )));
}

#[test]
fn texture_set_cache_keys_distinguish_same_named_images_in_different_packs() {
    let view = layered(
        &[
            ("textures/shared.png", &png(1, 1)),
            (
                "textures/alias.texture_set.json",
                br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":"shared"}}"#,
            ),
        ],
        &[("textures/shared.png", &png(2, 2))],
    );
    let policy = TexturePolicy::for_device(16);
    let alias = resolve_pack_texture_with_policy(&view, "textures/alias", policy, false).unwrap();
    let direct = resolve_pack_texture_with_policy(&view, "textures/shared", policy, false).unwrap();
    assert_eq!([alias.texture.width, direct.texture.width], [1, 2]);
    assert_ne!(alias.image_path, direct.image_path);
    for (path, image) in [("textures/alias", &alias), ("textures/shared", &direct)] {
        assert_eq!(
            indexed_image_candidate(&view, path).as_ref(),
            Some(&image.image_path)
        );
    }
    let explicit =
        resolve_pack_texture_with_policy(&view, "./textures/shared.PNG", policy, false).unwrap();
    assert_eq!(direct.image_path, explicit.image_path);
}

#[test]
fn extensionless_texture_decodes_tga_ahead_of_darker_png() {
    let mut png = Cursor::new(Vec::new());
    let mut tga = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([110, 110, 110, 255]))
        .write_to(&mut png, ImageFormat::Png)
        .unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([145, 145, 145, 255]))
        .write_to(&mut tga, ImageFormat::Tga)
        .unwrap();
    let view = LayeredPackView::new(crate::test_support::stack(&[
        ("textures/grass.png", png.get_ref()),
        ("textures/grass.tga", tga.get_ref()),
    ]));
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    let resolved =
        resolve_pack_texture_with_policy(&view, "textures/grass", policy, false).unwrap();
    assert_eq!(resolved.image_path, image_identity(0, "textures/grass.tga"));
    assert_eq!(&resolved.texture.rgba8[..4], &[145, 145, 145, 255]);
    let explicit =
        resolve_pack_texture_with_policy(&view, "textures/grass.png", policy, false).unwrap();
    assert_eq!(&explicit.texture.rgba8[..4], &[110, 110, 110, 255]);
}

#[test]
fn invalid_texture_sets_fall_back_to_the_plain_image_and_valid_sets_win() {
    let red = red_png();
    let view = LayeredPackView::new(crate::test_support::stack(&[
        ("textures/plain.png", &red),
        (
            "textures/plain.texture_set.json",
            br#"{"minecraft:texture_set":{"color":[1,2,3,255]}}"#,
        ),
        ("textures/set.png", &red),
        (
            "textures/set.texture_set.json",
            br##"{"format_version":"1.16.100","minecraft:texture_set":{"color":"#80102030"}}"##,
        ),
    ]));
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    let plain = resolve_pack_texture_with_policy(&view, "textures/plain", policy, false).unwrap();
    assert_eq!(
        plain.image_path,
        image_identity(0, "textures/plain.png"),
        "no format_version"
    );
    let set = resolve_pack_texture_with_policy(&view, "textures/set", policy, false).unwrap();
    assert_eq!(&set.texture.rgba8[..], &[0x10, 0x20, 0x30, 0x80]);
}

#[test]
fn the_highest_layer_holding_either_form_decides_between_image_and_texture_set() {
    let red = red_png();
    let set: &[u8] =
        br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":[1,2,3,255]}}"#;
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    let image_above = layered(
        &[("textures/blocks/stone.texture_set.json", set)],
        &[("textures/blocks/stone.png", red.as_slice())],
    );
    let resolved =
        resolve_pack_texture_with_policy(&image_above, "textures/blocks/stone", policy, false)
            .unwrap();
    assert_eq!(
        resolved.image_path,
        image_identity(0, "textures/blocks/stone.png")
    );
    assert_eq!(&resolved.texture.rgba8[..], &[255, 0, 0, 255]);
    assert_eq!(
        indexed_image_candidate(&image_above, "textures/blocks/stone"),
        Some(image_identity(0, "textures/blocks/stone.png"))
    );
    let set_above = layered(
        &[("textures/blocks/stone.png", red.as_slice())],
        &[("textures/blocks/stone.texture_set.json", set)],
    );
    let resolved =
        resolve_pack_texture_with_policy(&set_above, "textures/blocks/stone", policy, false)
            .unwrap();
    assert_eq!(&resolved.texture.rgba8[..], &[1, 2, 3, 255]);
    let same_layer = layered(
        &[],
        &[
            ("textures/blocks/stone.png", red.as_slice()),
            ("textures/blocks/stone.texture_set.json", set),
        ],
    );
    let resolved =
        resolve_pack_texture_with_policy(&same_layer, "textures/blocks/stone", policy, false)
            .unwrap();
    assert_eq!(&resolved.texture.rgba8[..], &[1, 2, 3, 255]);
}
