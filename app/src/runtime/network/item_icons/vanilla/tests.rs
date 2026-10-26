use super::*;
use assets::{
    BlockOverlay, Material, TextureRef,
    material_keys::{MaterialKeys, MaterialTextureSource},
};

/// A base carrier whose material 1 is a flat colour layer appended after the diagnostic page.
fn base(color: [u8; 4]) -> RuntimeAssets {
    let mut texture = RuntimeAssets::diagnostic().texture_array().clone();
    for mip in &mut texture.mips {
        mip.rgba8 = color.repeat((mip.size * mip.size) as usize).into();
    }
    let overlay = BlockOverlay {
        materials: vec![Material {
            texture: TextureRef::new(1, 0).unwrap(),
            ..Material::unvaried()
        }],
        textures: vec![texture],
        ..Default::default()
    };
    RuntimeAssets::diagnostic()
        .with_block_overlay(1, &overlay)
        .unwrap()
}

fn source(path: &str, baked_tint: Option<[u8; 3]>) -> MaterialTextureSource {
    MaterialTextureSource {
        key: "fixture".into(),
        variant: 0,
        path: path.into(),
        raster: format!("{path}.png").into(),
        baked_tint,
    }
}

/// Keys naming one material compiled from `path`, whose source image is `size` pixels.
fn keys(path: &str, size: [u32; 2], baked_tint: Option<[u8; 3]>) -> MaterialKeys {
    MaterialKeys::default()
        .with_sources(std::iter::empty(), [(1, source(path, baked_tint))], [])
        .with_raster_sizes([(format!("{path}.png").into_boxed_str(), size)])
}

#[test]
fn item_texture_paths_resolve_base_block_rasters() {
    let index = terrain_raster_index(&keys("textures/blocks/fixture_beacon", [16, 16], None));
    let texture = terrain_raster(
        &index,
        &base([9, 80, 160, 255]),
        "textures/blocks/Fixture_Beacon",
    )
    .expect("vanilla block raster by its item_texture path");
    assert_eq!((texture.width, texture.height), (16, 16));
    assert_eq!(&texture.rgba8[..4], &[9, 80, 160, 255]);
    assert!(terrain_raster(&index, &base([0; 4]), "textures/blocks/absent").is_none());
}

#[test]
fn only_images_the_carrier_keeps_unchanged_are_inherited() {
    for (size, baked_tint, why) in [
        (
            [32, 32],
            None,
            "a larger image is downsampled into the carrier tile",
        ),
        (
            [16, 64],
            None,
            "an animated strip keeps only one selected frame",
        ),
        (
            [16, 16],
            Some([120, 200, 80]),
            "a baked atlas tint changes the pixels",
        ),
    ] {
        let index = terrain_raster_index(&keys("textures/blocks/fixture", size, baked_tint));
        assert!(
            terrain_raster(&index, &base([1, 2, 3, 255]), "textures/blocks/fixture").is_none(),
            "{why}"
        );
    }
    let size_unknown = MaterialKeys::default().with_sources(
        std::iter::empty(),
        [(1, source("textures/blocks/fixture", None))],
        [],
    );
    assert!(terrain_raster_index(&size_unknown).is_empty());
}
