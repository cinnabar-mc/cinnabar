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

/// A one-colour 16x16 PNG.
fn flat_png(color: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(16, 16, image::Rgba(color));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn splash_route(metadata: u32, path: &str) -> Route {
    Route {
        identifier: Arc::from("minecraft:splash_potion"),
        metadata,
        path: format!("textures/items/{path}.png"),
        variant: metadata,
        aliases: vec!["splash_potion".into()],
    }
}

// A pack that replaces the plain splash bottle and healing must not lend that bottle to poison,
// which session lookups would otherwise reach through their variant-zero fallback.
#[test]
fn variants_a_pack_leaves_alone_keep_their_base_art_beside_a_replaced_variant_zero() {
    let view = super::super::tests::view_with(vec![
        (
            "textures/items/potion_bottle_splash.png",
            flat_png([200, 200, 200, 255]),
        ),
        (
            "textures/items/potion_bottle_splash_heal.png",
            flat_png([250, 30, 30, 255]),
        ),
    ]);
    let routes = [
        splash_route(0, "potion_bottle_splash"),
        splash_route(21, "potion_bottle_splash_heal"),
        splash_route(25, "potion_bottle_splash_poison"),
    ];
    let base = |path: &str| {
        (path == "textures/items/potion_bottle_splash_poison").then(|| DecodedTexture {
            width: 16,
            height: 16,
            rgba8: [134, 161, 98, 255].repeat(16 * 16).into(),
        })
    };
    let mut icons = Vec::new();
    let mut retained = 0;
    append_routes(&view, &routes, &mut icons, &mut retained);
    pin_base_variants(
        &routes,
        &mut icons,
        &mut retained,
        &HashSet::new(),
        usize::MAX,
        base,
    );
    let first_texel = |metadata: u32| {
        icons
            .iter()
            .find(|icon| icon.metadata == metadata)
            .map(|icon| <[u8; 4]>::try_from(&icon.rgba8[..4]).unwrap())
    };
    assert_eq!(first_texel(0), Some([200, 200, 200, 255]));
    assert_eq!(first_texel(21), Some([250, 30, 30, 255]));
    assert_eq!(first_texel(25), Some([134, 161, 98, 255]));
    assert_eq!(retained, icons.len() * 16 * 16 * 4);

    // Without a replaced variant zero, the base catalog draws every variant itself.
    let view = super::super::tests::view_with(vec![(
        "textures/items/potion_bottle_splash_heal.png",
        flat_png([250, 30, 30, 255]),
    )]);
    let mut icons = Vec::new();
    append_routes(&view, &routes, &mut icons, &mut 0);
    pin_base_variants(
        &routes,
        &mut icons,
        &mut 0,
        &HashSet::new(),
        usize::MAX,
        base,
    );
    assert_eq!(
        icons.iter().map(|icon| icon.metadata).collect::<Vec<_>>(),
        [21]
    );

    // An icon key's image covers every variant, so its variant zero keeps the fallback.
    let mut icons = vec![SessionIcon {
        identifier: Arc::from("minecraft:splash_potion"),
        metadata: 0,
        width: 16,
        height: 16,
        rgba8: [9, 9, 9, 255].repeat(16 * 16).into(),
    }];
    let keyed = HashSet::from([Arc::from("minecraft:splash_potion")]);
    pin_base_variants(&routes, &mut icons, &mut 0, &keyed, usize::MAX, base);
    assert_eq!(icons.len(), 1);

    // Pins stop at the limit, leaving room for what is staged after them.
    pin_base_variants(&routes, &mut icons, &mut 0, &HashSet::new(), 1, base);
    assert_eq!(icons.len(), 1);
    pin_base_variants(&routes, &mut icons, &mut 0, &HashSet::new(), 2, base);
    assert_eq!(icons.len(), 2);
}

/// The installed entity and icon carriers, or `None` when `make assets` has not produced them.
/// Unreadable or undecodable carriers fail the test rather than skipping it.
fn installed_item_carriers() -> Option<(RuntimeEntityAssets, assets::RuntimeIconCatalog)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/assets/compiled");
    let read = |name: &str| match std::fs::read(root.join(name)) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("reading installed carrier {name}: {error}"),
    };
    let (entities, icons) = (read("vanilla-v1.mcbeent")?, read("vanilla-v1.mcbeico")?);
    Some((
        RuntimeEntityAssets::decode(&entities).expect("installed entity carrier decodes"),
        assets::RuntimeIconCatalog::decode(&icons).expect("installed icon carrier decodes"),
    ))
}

// With only the plain bottles and arrow replaced, every potion, splash, lingering and tipped arrow
// variant still draws exactly the base art the vanilla catalog gives it.
#[test]
fn every_potion_variant_keeps_its_base_art_when_a_pack_replaces_the_plain_bottles() {
    let Some((entities, catalog)) = installed_item_carriers() else {
        eprintln!(
            "skipping every_potion_variant_keeps_its_base_art_when_a_pack_replaces_the_plain_bottles: missing installed item carriers fixture (make assets)"
        );
        return;
    };
    let sprites = base_sprites(&entities, &catalog).expect("matching item carriers");
    let routes = routes(&entities);
    let grey = flat_png([200, 200, 200, 255]);
    let view = super::super::tests::view_with(vec![
        ("textures/items/potion_bottle_drinkable.png", grey.clone()),
        ("textures/items/potion_bottle_splash.png", grey.clone()),
        ("textures/items/potion_bottle_lingering.png", grey.clone()),
        ("textures/items/arrow.png", grey),
    ]);
    let mut icons = Vec::new();
    append_routes(&view, &routes, &mut icons, &mut 0);
    pin_base_variants(
        &routes,
        &mut icons,
        &mut 0,
        &HashSet::new(),
        usize::MAX,
        |path| sprite_in(&sprites, path),
    );
    let refs = catalog.sprites();
    let (mut checked, mut shared) = (0, 0);
    for identifier in [
        "minecraft:potion",
        "minecraft:splash_potion",
        "minecraft:lingering_potion",
        "minecraft:arrow",
    ] {
        let replaced = icons
            .iter()
            .find(|icon| icon.identifier.as_ref() == identifier && icon.metadata == 0)
            .unwrap_or_else(|| panic!("{identifier} keeps the pack's variant zero"));
        assert_eq!(&replaced.rgba8[..4], &[200, 200, 200, 255]);
        let plain = &routes
            .iter()
            .find(|route| route.identifier.as_ref() == identifier && route.metadata == 0)
            .unwrap()
            .path;
        for route in routes
            .iter()
            .filter(|route| route.identifier.as_ref() == identifier && route.metadata != 0)
        {
            let icon = icons
                .iter()
                .find(|icon| {
                    icon.identifier.as_ref() == identifier && icon.metadata == route.metadata
                })
                .unwrap_or_else(|| panic!("{identifier}:{} has an icon", route.metadata));
            // Water, mundane, thick and awkward share the plain bottle, so they take its
            // replacement as the base game does.
            if route.path == *plain {
                assert_eq!(&icon.rgba8[..], &replaced.rgba8[..]);
                shared += 1;
                continue;
            }
            let base = &refs[catalog
                .lookup_index(identifier, route.metadata)
                .unwrap_or_else(|| panic!("{identifier}:{} base icon", route.metadata))];
            assert_eq!(
                (icon.width, icon.height, &*icon.rgba8),
                (u32::from(base.width), u32::from(base.height), &*base.rgba8),
                "{identifier}:{} draws its own base art",
                route.metadata
            );
            checked += 1;
        }
    }
    // Four base potions share the plain bottle in each form; the other 42 effect variants of
    // each form and all 64 tipped arrows keep their own art.
    assert_eq!((checked, shared), (42 * 3 + 64, 4 * 3));
}
