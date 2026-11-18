//! Controller-owned geometry must reach the near-camera pass, not only the world actor batch.
use super::*;
use bevy::math::Quat;
use serde_json::json;

/// Edits one original fixture document before the production pack compiler reads it.
fn edit_json(
    files: &mut [(Box<str>, Vec<u8>)],
    path: &str,
    edit: impl FnOnce(&mut serde_json::Value),
) {
    let (_, bytes) = files
        .iter_mut()
        .find(|(name, _)| name.as_ref() == path)
        .unwrap();
    let mut value = serde_json::from_slice(bytes).unwrap();
    edit(&mut value);
    *bytes = serde_json::to_vec(&value).unwrap();
}

#[test]
fn first_person_controller_geometry_reaches_hand_publication() {
    for alpha in [0.5, 1.0] {
        assert_controller_hand_publication(alpha);
    }
}

/// Checks both interpolated and completed samples through actual hand publication.
fn assert_controller_hand_publication(alpha: f32) {
    let ((stream, mut equipment, artwork, input), entities) = native_tests::fixture_with_files(
        None,
        false,
        false,
        true,
        false,
        false,
        None,
        |files| {
            edit_json(files, "entity/player.json", |value| {
                let description = &mut value["minecraft:client_entity"]["description"];
                description["geometry"]["extra"] = "geometry.fixture_extra".into();
                description["textures"]["extra"] = "textures/fixture_extra".into();
                description["materials"]["extra"] = "entity_alphablend".into();
                description["animations"]["extra"] = "animation.fixture_extra.pose".into();
                description["scripts"]["animate"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("extra"));
                description["render_controllers"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"controller.render.fixture_extra":"variable.is_first_person"}));
            });
            edit_json(files, "render_controllers/test.json", |value| {
                value["render_controllers"]["controller.render.fixture_extra"] = json!({
                    "geometry":"Geometry.extra", "textures":["Texture.extra"],
                    "materials":[{"*":"Material.extra"}],
                });
            });
            files.push(("models/entity/fixture_extra.geo.json".into(), serde_json::to_vec(&json!({
                "format_version":"1.12.0", "minecraft:geometry":[{
                    "description":{"identifier":"geometry.fixture_extra","texture_width":16,"texture_height":16},
                    "bones":[{"name":"extra_root","pivot":[0,0,0],"cubes":[{
                        "origin":[0,20,-4],"size":[2,2,4],"uv":[0,0]
                    }]}]
                }]
            })).unwrap()));
            files.push((
                "animations/fixture_extra.animation.json".into(),
                serde_json::to_vec(&json!({
                    "format_version":"1.8.0", "animations":{"animation.fixture_extra.pose":{
                        "loop":true,"bones":{"extra_root":{"rotation":[0,35,0]}}
                    }}
                }))
                .unwrap(),
            ));
            let mut png = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(16, 16, image::Rgba([32, 224, 64, 192]))
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            files.push(("textures/fixture_extra.png".into(), png.into_inner()));
        },
    );
    let mut sampled = stream.authority().actor_render_frame(alpha);
    let layers = sampled
        .layers(1)
        .expect("the player has sampled render controllers");
    let layer = layers
        .iter()
        .find(|layer| layer.geometry.is_some())
        .expect("the first-person controller selects its separate geometry")
        .clone();
    let body_layer = layers
        .iter()
        .find(|layer| layer.geometry.is_none())
        .unwrap();
    assert_ne!(
        layer.render_state, body_layer.render_state,
        "the model has its own raster state"
    );
    assert!(
        layer.render_state.unwrap().blend.is_some(),
        "the authored model is blended"
    );
    assert_eq!(
        layer.pose.len(),
        1,
        "the model owns a separate one-bone skeleton"
    );
    assert!(
        !Quat::from_array(layer.pose[0].rotation).abs_diff_eq(Quat::IDENTITY, 1e-5),
        "the controller model has a non-rest sampled pose"
    );
    let location = artwork
        .source_location(layer.pack, layer.source)
        .expect("the actual compiled model texture is available before hand selection");
    assert_ne!(
        location,
        artwork
            .source_location(body_layer.pack, body_layer.source)
            .unwrap()
    );
    let catalog_rig = if layer.pack {
        render_model::pack_rig_id(0)
    } else {
        render_model::EntityRigId(0)
    };
    let expected_rig = render_model::layer_geometry_rig_id(catalog_rig, layer.geometry.unwrap());
    let mut source_cache = java::HandCache::default();
    let hand = native_tests::hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut source_cache,
        alpha,
        false,
    );
    let repeated = native_tests::hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut source_cache,
        alpha,
        false,
    );
    assert_eq!(hand.controller_draws, repeated.controller_draws);
    let body = hand
        .body
        .as_ref()
        .expect("the ordinary arm remains available");
    let controller_draw = hand
        .controller_draws
        .iter()
        .find(|(submission, _)| submission.input.rig == expected_rig)
        .expect("the sampled controller model is selected into hand extras");
    assert_eq!(controller_draw.1, Some(location));
    let body_geometry = render_model::geometry_from_runtime_assets(&entities, 0)
        .unwrap()
        .with_id(body.input.rig);
    let mut geometries = vec![body_geometry];
    geometries.extend(render_model::layer_geometries(&entities, catalog_rig));
    let mut builder = ActorRigFrameBuilder::new(geometries).unwrap();
    assert!(
        builder.can_draw_submission(body),
        "the base arm is registered, not a missing-fixture failure"
    );
    let mut scene = HandRigScene::default();
    let mut revision = 0;
    publish_hand_rig(
        &mut builder,
        &mut scene,
        &mut revision,
        Some(hand),
        &artwork,
        Some(90f32.to_radians()),
        HandRigLight::default(),
        (alpha, None),
    );
    let frame = scene
        .frame
        .expect("the ordinary hand publishes successfully");
    let entry = frame
        .rig
        .manifest
        .iter()
        .find(|entry| entry.rig == expected_rig)
        .expect("the sampled controller model must reach the published first-person hand frame");
    let instance = &frame.rig.instances[entry.instance_index as usize];
    assert_eq!(
        frame.draw_locations[entry.instance_index as usize],
        Some(location)
    );
    let retained_artwork = frame
        .artwork
        .as_ref()
        .expect("hand frame retains its artwork pages");
    let page_index = usize::from(location.page()) - 1;
    assert!(
        retained_artwork.pages()[page_index]
            .shared_pixels()
            .ptr_eq(&artwork.pages()[page_index].shared_pixels()),
        "retained hand artwork shares page storage rather than copying pixels"
    );
    assert_ne!(
        instance.material & assets::EntityRenderMaterialState::BLEND,
        0,
        "the model retains its own authored material, not the body material"
    );
    assert_eq!(
        frame.rig.rasters[instance.raster as usize],
        layer.render_state.unwrap()
    );
    let actual = frame.rig.current_bones[entry.current_bone_base as usize];
    let expected = Mat4::from_quat(Quat::from_array(layer.pose[0].rotation))
        .transpose()
        .to_cols_array_2d();
    for row in 0..3 {
        for column in 0..4 {
            assert!(
                (actual[row][column] - expected[row][column]).abs() < 1e-5,
                "the published model retains its independent sampled pose"
            );
        }
    }
}

/// A held definition that keeps its geometry, controller and texture alias but resolves no base
/// raster must not add an ordinary item sprite beside its empty attachable draw. Two fixtures
/// share the item, icon, actor feed and compiled geometry/controller; only the binding differs.
#[test]
fn sampled_attachable_without_base_artwork_suppresses_ordinary_sprite() {
    // M: the shield attachable still binds the held item and keeps its geometry, controller and
    // `Texture.default` alias, but that alias names a PNG the production pack compiler cannot
    // resolve. This is a compiled pack route, not an icon-only fallback, so the empty base is
    // really compiled.
    let ((stream, mut equipment, artwork, input), matched_entities) =
        native_tests::fixture_with_files(
            Some("minecraft:shield"),
            false,
            false,
            true,
            false,
            false,
            None,
            |files| {
                edit_json(files, "attachables/shield.json", |value| {
                    value["minecraft:attachable"]["description"]["textures"]["default"] =
                        json!("textures/missing_held_artwork");
                });
            },
        );
    let mut matched_cache = java::HandCache::default();
    let matched = native_tests::hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut matched_cache,
        1.0,
        false,
    );

    // U: identical held shield, icon, feed and compiled geometry/controller, but this attachable
    // binds a different item, so no definition is admitted for the held shield and the ordinary
    // sprite is the only valid draw. Rebuilding the fixture keeps the actor feed in sync: the
    // cache gates the displayed stack on `rig.main_hand_equipped`, so swapping only `input.main`
    // would drop the displayed item before any sprite could be selected.
    let (
        (ordinary_stream, mut ordinary_equipment, _ordinary_artwork, ordinary_input),
        ordinary_entities,
    ) = native_tests::fixture_with_files(
        Some("minecraft:shield"),
        false,
        false,
        true,
        false,
        false,
        None,
        |files| {
            edit_json(files, "attachables/shield.json", |value| {
                value["minecraft:attachable"]["description"]["identifier"] =
                    json!("minecraft:shield_unbound");
            });
        },
    );
    let mut ordinary_cache = java::HandCache::default();
    let ordinary = native_tests::hand_source_for_mode(
        &ordinary_stream,
        &mut ordinary_equipment,
        &_ordinary_artwork,
        &ordinary_input,
        &mut ordinary_cache,
        1.0,
        false,
    );

    // Control first: with no attachable admitted for the held item, the ordinary layer exists.
    assert!(
        ordinary.items[0].is_some(),
        "an item with no admitted attachable keeps its ordinary held sprite"
    );

    // Public preconditions, read from the compiled catalogs without a new production API: the
    // sampled fixture admits this attachable with valid geometry/controller, and its base alias
    // stays authored while the PNG is absent from the pack. The control binds another item.
    fn compiled_symbol<'a>(
        entities: &'a std::sync::Arc<assets::RuntimeEntityAssets>,
        identifier: &str,
    ) -> Option<&'a assets::EntityAssetSymbol> {
        entities
            .symbols()
            .iter()
            .find(|symbol| symbol.identifier.as_ref() == identifier)
    }
    assert!(
        compiled_symbol(&ordinary_entities, "minecraft:shield").is_none(),
        "the control must not admit an attachable for the held shield"
    );
    let sampled = compiled_symbol(&matched_entities, "minecraft:shield")
        .expect("the sampled fixture must admit an attachable for the held shield");
    let resolved = |kind: assets::EntityDependencyKind, identifier: &str| {
        sampled.dependencies.iter().any(|dependency| {
            dependency.kind == kind
                && dependency.identifier.as_ref() == identifier
                && dependency.resolution == assets::EntityDependencyResolution::Catalog
        })
    };
    assert!(
        resolved(assets::EntityDependencyKind::Geometry, "geometry.item_test")
            && resolved(
                assets::EntityDependencyKind::RenderController,
                "controller.render.test"
            ),
        "the sampled attachable keeps valid geometry and a render controller"
    );
    assert!(
        sampled.dependencies.iter().any(|dependency| {
            dependency.kind == assets::EntityDependencyKind::Texture
                && dependency.identifier.as_ref() == "textures/missing_held_artwork"
                && dependency.resolution == assets::EntityDependencyResolution::External
        }),
        "the base alias is authored but its PNG is absent from the compiled pack"
    );

    // Behavior under test: an admitted attachable with no base raster must not fall back to a
    // second, ordinary sprite.
    assert!(
        matched.items[0].is_none(),
        "a sampled attachable with missing base artwork must not emit an ordinary sprite"
    );
    assert!(
        matched.body.is_some(),
        "a missing held-item draw must leave the first-person arm available"
    );
}

/// A startup binding admitted only because its geometry is external, with no authored
/// attachable definition, must leave the ordinary held sprite in place. Same held feed,
/// inventory and icon path as the production hand fixture; only the catalog binding differs.
#[test]
fn geometry_only_startup_fallback_keeps_ordinary_sprite() {
    const HELD: &str = "minecraft:diamond_sword";
    const EXTERNAL: &str = "geometry.startup_fallback_external";
    let ((stream, mut equipment, artwork, input), entities, catalog) =
        native_tests::fixture_with_catalog(
            Some(HELD),
            false,
            false,
            true,
            false,
            false,
            None,
            |_| {},
            |bindings| {
                let mut binding = bindings
                    .iter()
                    .find(|binding| binding.identifier.as_ref() == "minecraft:shield")
                    .unwrap()
                    .clone();
                binding.identifier = HELD.into();
                binding.attachable = None;
                binding.geometry.identifier = EXTERNAL.into();
                binding.geometry.resolution = assets::EntityDependencyResolution::External;
                bindings.push(binding);
                bindings.sort_by(|left, right| left.identifier.cmp(&right.identifier));
            },
        );

    // The production catalog admits this binding without a definition that could evaluate.
    let binding = catalog
        .bindings_for(HELD)
        .iter()
        .find(|binding| {
            binding.attachable.is_none()
                && binding.geometry.resolution == assets::EntityDependencyResolution::External
                && binding.geometry.identifier.as_ref() == EXTERNAL
        })
        .expect("the startup catalog must carry the external fallback binding");
    assert_eq!(binding.identifier.as_ref(), HELD);
    assert!(
        entities
            .attachable_rig_binding(binding.attachable_identifier())
            .is_none(),
        "there is no authored attachable definition for the fallback binding"
    );

    let mut cache = java::HandCache::default();
    let hand = native_tests::hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        1.0,
        false,
    );
    assert!(
        hand.items[0].is_some(),
        "an admitted geometry-only startup fallback must keep the ordinary held sprite"
    );
    #[cfg(feature = "developer-control")]
    assert_eq!(
        hand.item_selection[0].as_ref().unwrap().branch,
        "ordinary_after_no_attachable_draw",
        "the fallback has no authored draw to suppress the sprite"
    );
}
