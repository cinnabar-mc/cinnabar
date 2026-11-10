//! Texture-only reloads update artwork paths without introducing pack bindings.

use super::*;

const ITEM: &str = "fixture:chestplate";
const TEXTURE: &str = "textures/items/fixture_armor";
/// A validated startup artwork table with no inherited pack bindings.
fn base_catalog() -> Arc<assets::RuntimeEquipmentCatalog> {
    Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [1; 32],
            Vec::new(),
            vec![assets::EquipmentTexture {
                identifier: TEXTURE.into(),
                width: 4,
                height: 2,
                rgba8: Arc::from([10, 20, 30, 255].repeat(8)),
            }],
        )
        .unwrap(),
    )
}

/// A stack with the same pack and texture names but independently authored pixels.
fn texture_stack(color: [u8; 4]) -> Arc<resource_pack::ValidatedPackStack> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 2, image::Rgba(color))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    crate::test_support::stack(&[(&format!("{TEXTURE}.png"), png.get_ref())])
}

/// Compiles on the same cache used by session preparation, retaining its dependency reads.
fn resolve(
    cache: &EntityCache,
    stack: &Arc<resource_pack::ValidatedPackStack>,
    catalog: &Arc<assets::RuntimeEquipmentCatalog>,
) -> (
    Option<Arc<SessionEntityPack>>,
    crate::reload_diff::Dependencies,
) {
    let mut dependencies = Default::default();
    let pack = crate::reload_diff::compile(
        crate::reload_diff::Subscriber::Entities,
        stack,
        &mut dependencies,
        |view| {
            cache
                .get_or_compile(
                    EntityInputs {
                        stack: crate::stack_fingerprint(stack),
                        vanilla: None,
                        equipment: Some(catalog.clone()),
                        vanilla_pack_dir: None,
                        fancy_graphics: true,
                    },
                    view,
                    None,
                    &|| false,
                    super::super::compile_encoded,
                )
                .expect("the compile was not cancelled")
        },
    );
    (pack, dependencies)
}

#[test]
fn a_texture_only_reload_changes_artwork_for_the_same_equipped_item() {
    let (catalog, cache) = (base_catalog(), EntityCache::default());
    let equipped = ITEM;
    let first_stack = texture_stack([21, 43, 65, 255]);
    let (first, reads) = resolve(&cache, &first_stack, &catalog);
    let first = first.expect("texture-only armour retains artwork without pack bindings");
    let first_catalog = first.equipment.as_ref().unwrap();
    assert!(first_catalog.binding(equipped).is_none());
    let first_pixels = first_catalog.texture(TEXTURE).unwrap();
    assert_eq!(&first_pixels.rgba8[..4], &[21, 43, 65, 255]);
    let (unchanged, _) = resolve(&cache, &first_stack, &catalog);
    assert!(Arc::ptr_eq(&first, &unchanged.unwrap()));

    let next_stack = texture_stack([87, 109, 131, 255]);
    let changes = crate::reload_diff::Changes::between(
        &next_stack,
        Some((
            &resource_pack::PackAdmission::Validated(first_stack),
            &reads,
        )),
    );
    assert!(changes.entities, "changed pixels must refresh equipment");
    let (next, _) = resolve(&cache, &next_stack, &catalog);
    let next = next.unwrap();
    assert!(!Arc::ptr_eq(&first, &next));
    let next_catalog = next.equipment.as_ref().unwrap();
    assert!(next_catalog.binding(equipped).is_none());
    assert_ne!(
        next_catalog.source_manifest_sha256(),
        first_catalog.source_manifest_sha256()
    );
    assert_eq!(
        &next_catalog.texture(TEXTURE).unwrap().rgba8[..4],
        &[87, 109, 131, 255]
    );
    assert_eq!(&first_pixels.rgba8[..4], &[21, 43, 65, 255]);
    let empty = crate::test_support::stack(&[]);
    let removed = crate::reload_diff::Changes::between(
        &empty,
        Some((&resource_pack::PackAdmission::Validated(next_stack), &reads)),
    );
    assert!(removed.entities);
    assert!(resolve(&cache, &empty, &catalog).0.is_none());
}

#[test]
fn artwork_only_layers_do_not_create_attachable_rigs() {
    let catalog = base_catalog();
    let stack = texture_stack([21, 43, 65, 255]);
    let pack = resolve(&EntityCache::default(), &stack, &catalog)
        .0
        .unwrap();
    assert!(pack.assets.rig_bindings().is_empty());
    assert!(pack.assets.symbols().is_empty());
    assert!(pack.equipment.as_ref().unwrap().bindings().is_empty());
    let encoded = pack.encode(&pack.assets.encode().unwrap()).unwrap();
    let restored = SessionEntityPack::decode(&encoded).unwrap();
    assert!(pack.material_report.is_some());
    assert_eq!(
        restored.material_report, pack.material_report,
        "a cache-restored pack reports its compile's counts"
    );
    assert!(restored.assets.rig_bindings().is_empty());
    assert!(restored.assets.symbols().is_empty());
    let artwork = restored.equipment.unwrap();
    assert!(artwork.bindings().is_empty());
    assert_eq!(
        &artwork.texture(TEXTURE).unwrap().rgba8[..4],
        &[21, 43, 65, 255]
    );
}

/// Creates independently allocated actor and equipment rasters with identical bytes.
fn overlapping_artwork_pack() -> Arc<SessionEntityPack> {
    use sha2::{Digest, Sha256};
    let catalog = base_catalog();
    let stack = texture_stack([21, 43, 65, 255]);
    let original = resolve(&EntityCache::default(), &stack, &catalog)
        .0
        .unwrap();
    let equipment = &original.equipment.as_ref().unwrap().textures()[0];
    Arc::new(SessionEntityPack {
        textures: vec![assets::ActorTexture {
            source: 0,
            width: equipment.width,
            height: equipment.height,
            pixel_sha256: Sha256::digest(&equipment.rgba8).into(),
            rgba8: Arc::from(equipment.rgba8.as_ref()),
        }]
        .into(),
        ..original.as_ref().clone()
    })
}

#[test]
fn a_fresh_compile_shares_equal_actor_and_equipment_pixels_without_changing_encoding() {
    let original = overlapping_artwork_pack();
    let blob = original.assets.encode().unwrap();
    let encoded = original.encode(&blob).unwrap();
    let cache = EntityCache::default();
    let view = resource_pack::LayeredPackView::new(crate::test_support::stack(&[]));
    let next = cache
        .get_or_compile(
            EntityInputs {
                stack: Vec::new(),
                vanilla: None,
                equipment: None,
                vanilla_pack_dir: None,
                fancy_graphics: true,
            },
            &view,
            None,
            &|| false,
            |_, _, _, _, _, _, _| Some((Some(Arc::clone(&original)), None)),
        )
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(
        &next.textures[0].rgba8,
        &next.equipment.as_ref().unwrap().textures()[0].rgba8
    ));
    assert!(!Arc::ptr_eq(
        &original.textures[0].rgba8,
        &original.equipment.as_ref().unwrap().textures()[0].rgba8
    ));
    assert_eq!(next.encode(&blob).unwrap(), encoded);
    let decoded = SessionEntityPack::decode(&encoded).unwrap();
    assert!(Arc::ptr_eq(
        &decoded.textures[0].rgba8,
        &decoded.equipment.as_ref().unwrap().textures()[0].rgba8
    ));
    assert_eq!(decoded.encode(&blob).unwrap(), encoded);
}

#[test]
fn equal_digests_cannot_share_different_pixel_bytes() {
    let original = overlapping_artwork_pack();
    let mut forged = original.textures[0].clone();
    let mut pixels = forged.rgba8.to_vec();
    pixels[0] ^= 1;
    forged.rgba8 = pixels.into();
    let original = Arc::new(SessionEntityPack {
        textures: vec![forged].into(),
        ..original.as_ref().clone()
    });
    let shared = Arc::clone(&original).with_shared_equipment_pixels();
    assert!(Arc::ptr_eq(&original, &shared));
    assert_ne!(
        shared.textures[0].rgba8,
        shared.equipment.as_ref().unwrap().textures()[0].rgba8
    );
}

/// A join without packs records no layer listings, yet a later entity pack must recompile.
#[test]
fn an_entity_pack_added_to_a_packless_session_recompiles_entities() {
    let (catalog, cache) = (base_catalog(), EntityCache::default());
    let packless = resource_pack::validate_handoff(protocol::ResourcePackHandoff::default());
    let (_, reads) = resolve(&cache, &packless, &catalog);
    let added = crate::test_support::stack(&[(
        "entity/fixture.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:actor"}}}"#,
    )]);
    let previous = resource_pack::PackAdmission::Validated(packless);
    assert!(crate::reload_diff::Changes::between(&added, Some((&previous, &reads))).entities);
}

/// A one-entity stack drawing `material` from the given `materials/entity.material` text.
fn material_stack(material: &[u8]) -> Arc<resource_pack::ValidatedPackStack> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([90, 90, 90, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
        "identifier":"example:moth","materials":{"default":"example:glow"},
        "textures":{"default":"textures/entity/moth"},"geometry":{"default":"geometry.moth"},
        "render_controllers":["controller.render.moth"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.moth","texture_width":16,"texture_height":16},
        "bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[4,4,4],"uv":[0,0]}]}]}]}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.moth":{
        "geometry":"Geometry.default","materials":[{"*":"Material.default"}],
        "textures":["Texture.default"]}}}"#;
    crate::test_support::stack(&[
        ("entity/moth.entity.json", entity),
        ("models/entity/moth.geo.json", geometry),
        ("render_controllers/moth.json", controller),
        ("materials/entity.material", material),
        ("textures/entity/moth.png", png.get_ref()),
    ])
}

/// The defines the session pack's `example:glow` draws, if the pack draws it.
fn drawn_defines(pack: &SessionEntityPack) -> Option<Vec<String>> {
    pack.material_report
        .as_ref()?
        .materials
        .iter()
        .find(|material| material.name.as_ref() == "example:glow")
        .map(|material| {
            material
                .applied_defines
                .iter()
                .chain(material.carried_defines.iter())
                .map(ToString::to_string)
                .collect()
        })
}

#[test]
fn a_malformed_material_file_and_its_removal_leave_no_stale_material() {
    let (catalog, cache) = (base_catalog(), EntityCache::default());
    let valid = |define: &str| {
        format!(
            r#"{{"materials":{{"version":"1.0.0","example:glow:entity_alphatest":{{"+defines":["{define}"]}}}}}}"#
        )
    };
    let (first, _) = resolve(&cache, &material_stack(valid("FIRST").as_bytes()), &catalog);
    assert!(
        drawn_defines(&first.unwrap())
            .unwrap()
            .contains(&"FIRST".to_owned())
    );
    // A malformed file mid-session is skipped; the entity still compiles without it.
    let (malformed, _) = resolve(&cache, &material_stack(b"{ not json"), &catalog);
    let malformed = malformed.expect("a malformed material file never drops the pack");
    assert!(
        drawn_defines(&malformed).is_none_or(|defines| !defines.contains(&"FIRST".to_owned())),
        "the earlier generation's material does not survive"
    );
    // Removing the pack leaves nothing behind.
    let (removed, _) = resolve(&cache, &crate::test_support::stack(&[]), &catalog);
    assert!(removed.is_none_or(|pack| {
        pack.material_report
            .as_ref()
            .is_none_or(|report| report.materials.is_empty())
    }));
}

#[test]
fn back_to_back_generations_publish_the_latest_materials() {
    let (catalog, cache) = (base_catalog(), EntityCache::default());
    let mut last = None;
    for define in ["ONE", "TWO", "THREE", "FOUR"] {
        let material = format!(
            r#"{{"materials":{{"version":"1.0.0","example:glow:entity_alphatest":{{"+defines":["{define}"]}}}}}}"#
        );
        let (pack, _) = resolve(&cache, &material_stack(material.as_bytes()), &catalog);
        last = pack;
    }
    let defines = drawn_defines(&last.unwrap()).unwrap();
    assert!(defines.contains(&"FOUR".to_owned()));
    for stale in ["ONE", "TWO", "THREE"] {
        assert!(!defines.contains(&stale.to_owned()), "{stale} survived");
    }
}
