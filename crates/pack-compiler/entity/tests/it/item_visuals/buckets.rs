use super::*;

#[test]
fn legacy_bucket_aux_selects_the_matching_named_bucket_artwork() {
    let pack = item_pack(false);
    let contents = [
        "empty", "milk", "water", "lava", "cod", "salmon", "tropical", "puffer", "snow", "axolotl",
        "tadpole", "sulfur",
    ];
    let paths = contents.map(|name| format!("textures/items/fixture_{name}"));
    write(
        pack.path(),
        "textures/item_texture.json",
        &serde_json::to_vec(&serde_json::json!({
            "texture_data": {"bucket": {"textures": paths}}
        }))
        .unwrap(),
    );
    for (index, path) in paths.iter().enumerate() {
        image::RgbaImage::from_pixel(2, 2, image::Rgba([index as u8, 57, 103, 255]))
            .save(pack.path().join(format!("{path}.png")))
            .unwrap();
    }
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let compiled = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let icons = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    for (metadata, named) in [
        (0, "minecraft:bucket"),
        (1, "minecraft:milk_bucket"),
        (2, "minecraft:cod_bucket"),
        (3, "minecraft:salmon_bucket"),
        (4, "minecraft:tropical_fish_bucket"),
        (5, "minecraft:pufferfish_bucket"),
        (8, "minecraft:water_bucket"),
        (10, "minecraft:lava_bucket"),
        (11, "minecraft:powder_snow_bucket"),
    ] {
        assert_eq!(
            visual(&entity.item_visuals, "minecraft:bucket", metadata).route,
            visual(&entity.item_visuals, named, 0).route,
            "bucket aux {metadata} must select {named}"
        );
        assert_eq!(
            icons.lookup("minecraft:bucket", metadata),
            icons.lookup(named, 0),
            "bucket aux {metadata} must retain the named bucket's pixels"
        );
    }
}
