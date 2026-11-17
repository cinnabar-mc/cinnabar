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
