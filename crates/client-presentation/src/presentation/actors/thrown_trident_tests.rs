//! Original thrown-trident fixtures for the native flight placement and foil.

use super::super::{ModelFrame, sampled_rig_placement};
use super::*;
use actor_animation::ActorRigSnapshot;
use image::ImageEncoder;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render_model::ActorArtworkPages;
use std::sync::Arc;

const POSITION: [f32; 3] = [1.0, 2.0, 3.0];
/// Authored heights of the fixture's head and butt; the native origin is at 24 pixels.
const HEAD_PIXELS: f32 = 28.0;
const BUTT_PIXELS: f32 = -3.0;

fn world(yaw: f32, pitch: f32) -> client_world::WorldAuthority {
    world_at(POSITION, yaw, pitch)
}

fn world_at(position: [f32; 3], yaw: f32, pitch: f32) -> client_world::WorldAuthority {
    world_with(position, yaw, pitch, false)
}

/// Compiles an original pole model, with its pivot at the native origin, without game assets.
/// With `long_choice`, the base pole declares a tiny visibility box and a render controller
/// selects a six-block pole whose upright box only covers it standing up.
fn world_with(
    position: [f32; 3],
    yaw: f32,
    pitch: f32,
    long_choice: bool,
) -> client_world::WorldAuthority {
    let identifier = NativeEntityClass::ThrownTrident.identifier().unwrap();
    let mut entity: serde_json::Value = serde_json::from_str(&format!(
        r#"{{"format_version":"1.8.0","minecraft:client_entity":{{"description":{{
        "identifier":"{identifier}","textures":{{"default":"textures/entity/pole"}},
        "geometry":{{"default":"geometry.pole"}}}}}}}}"#
    ))
    .unwrap();
    let mut geometry: serde_json::Value = serde_json::from_slice(
        br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{
        "identifier":"geometry.pole","texture_width":4,"texture_height":32},
        "bones":[{"name":"pole","pivot":[0,24,0],"cubes":[{"origin":[-0.5,-3,-0.5],
        "size":[1,31,1],"uv":[0,0]}]}]}]}"#,
    )
    .unwrap();
    let mut texture = Vec::new();
    image::codecs::png::PngEncoder::new(&mut texture)
        .write_image(&[40, 120, 110, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let mut resources = vec![("textures/entity/pole.png".into(), texture)];
    if long_choice {
        let description = &mut entity["minecraft:client_entity"]["description"];
        description["materials"] = serde_json::json!({"default": "entity"});
        description["geometry"]["long"] = "geometry.long".into();
        description["render_controllers"] = serde_json::json!(["controller.render.pole"]);
        let base = &mut geometry["minecraft:geometry"][0];
        let mut long = base.clone();
        base["description"]["visible_bounds_width"] = 0.1.into();
        base["description"]["visible_bounds_height"] = 0.1.into();
        base["description"]["visible_bounds_offset"] = serde_json::json!([0, 0, 0]);
        long["description"]["identifier"] = "geometry.long".into();
        long["description"]["visible_bounds_width"] = 1.into();
        long["description"]["visible_bounds_height"] = 6.into();
        long["description"]["visible_bounds_offset"] = serde_json::json!([0, -1.5, 0]);
        long["bones"][0]["cubes"][0]["origin"] = serde_json::json!([-0.5, -72, -0.5]);
        long["bones"][0]["cubes"][0]["size"] = serde_json::json!([1, 96, 1]);
        geometry["minecraft:geometry"]
            .as_array_mut()
            .unwrap()
            .push(long);
        resources.push((
            "render_controllers/pole.json".into(),
            br#"{"format_version":"1.8.0","render_controllers":{"controller.render.pole":{
            "geometry":"Geometry.long","materials":[{"*":"Material.default"}],
            "textures":["Texture.default"]}}}"#
                .to_vec(),
        ));
    }
    resources.push((
        "entity/pole.json".into(),
        serde_json::to_vec(&entity).unwrap(),
    ));
    resources.push((
        "models/entity/pole.geo.json".into(),
        serde_json::to_vec(&geometry).unwrap(),
    ));
    let compiled = pack_entity_compiler::compile_entity_pack(resources)
        .unwrap()
        .unwrap();
    let mut world = client_world::WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 999,
            local_player_unique_id: 999,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Some(Arc::new(
            assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap(),
        )),
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: ActorKind::Entity {
                    identifier: identifier.into(),
                },
                position,
                velocity: [0.0; 3],
                pitch,
                yaw,
                head_yaw: yaw,
                body_yaw: yaw,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    world.advance_actor_interpolation_ticks(3);
    world
}

fn transform(rows: &[[f32; 4]; 3], point: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        rows[row][0] * point[0] + rows[row][1] * point[1] + rows[row][2] * point[2] + rows[row][3]
    })
}

/// A projectile's flight direction for its yaw (`atan2(x, z)`) and pitch (`atan2(y, h)`).
fn flight(yaw: f32, pitch: f32) -> Vec3 {
    let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
    Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

#[test]
fn thrown_tridents_point_their_head_along_their_flight_from_their_position() {
    for (yaw, pitch) in [(0.0, 0.0), (90.0, 0.0), (-135.0, 30.0), (20.0, -60.0)] {
        let world = world(yaw, pitch);
        let rig = world
            .actor_rig(1)
            .expect("the thrown trident has a body route");
        let actor = world.actor(1).unwrap().clone();
        let rows = sampled_rig_placement(&rig, &actor, 0.5, [1.0; 4])
            .unwrap()
            .world_from_model;
        let point = |pixels: f32| Vec3::from_array(transform(&rows, [0.0, pixels / 16.0, 0.0]));
        let position = Vec3::from_array(POSITION);
        let direction = flight(yaw, pitch);
        let head = position + direction * (HEAD_PIXELS - 24.0) / 16.0;
        let butt = position + direction * (BUTT_PIXELS - 24.0) / 16.0;
        assert!(
            point(HEAD_PIXELS).abs_diff_eq(head, 1e-5),
            "yaw {yaw} pitch {pitch}: head {} != {head}",
            point(HEAD_PIXELS)
        );
        assert!(
            point(BUTT_PIXELS).abs_diff_eq(butt, 1e-5),
            "yaw {yaw} pitch {pitch}: butt {} != {butt}",
            point(BUTT_PIXELS)
        );
    }
}

#[test]
fn an_authored_y_axis_scale_keeps_the_native_pivot_on_the_actor() {
    for (yaw, pitch) in [(0.0, 0.0), (-135.0, 30.0)] {
        let world = world(yaw, pitch);
        let rig = world.actor_rig(1).unwrap();
        let actor = world.actor(1).unwrap().clone();
        let rows = sampled_rig_placement(&rig, &actor, 0.5, [1.0, 1.0, 2.0, 1.0])
            .unwrap()
            .world_from_model;
        let pivot = Vec3::from_array(transform(&rows, [0.0, 24.0 / 16.0, 0.0]));
        assert!(
            pivot.abs_diff_eq(Vec3::from_array(POSITION), 1e-5),
            "yaw {yaw} pitch {pitch}: pivot at {pivot}"
        );
        let lit = Vec3::from_array(ModelFrame::of(&actor).actor_position(&rows));
        assert!(
            lit.abs_diff_eq(Vec3::from_array(POSITION), 1e-5),
            "yaw {yaw} pitch {pitch}: lit at {lit}"
        );
        let head = Vec3::from_array(transform(&rows, [0.0, HEAD_PIXELS / 16.0, 0.0]));
        let expected = Vec3::from_array(POSITION) + flight(yaw, pitch) * 2.0 * 4.0 / 16.0;
        assert!(
            head.abs_diff_eq(expected, 1e-5),
            "yaw {yaw} pitch {pitch}: head {head} != {expected}"
        );
    }
}

#[test]
fn early_admission_keeps_a_level_trident_whose_shaft_reaches_into_view() {
    let camera = Vec3::new(0.0, 2.0, 10.0);
    let view = render_model::ActorCullView {
        camera_position: camera,
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            90_f32.to_radians(),
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_to_mat4(camera, -Vec3::Z, Vec3::Y),
        max_distance: 100.0,
    };
    // Seven blocks ahead, the right frustum edge is at x = 7, and the upright fallback box
    // at x = 8.4 lies wholly outside it. Flying toward +X, the trident's shaft trails 27
    // pixels back from its position, into view.
    for (x, visible) in [(8.4, true), (14.0, false)] {
        let world = world_at([x, 2.0, 3.0], 90.0, 0.0);
        let rig = world.actor_rig(1).unwrap();
        let actor = world.actor(1).unwrap();
        let rows = sampled_rig_placement(&rig, actor, 0.5, [1.0; 4])
            .unwrap()
            .world_from_model;
        let butt = transform(&rows, [0.0, BUTT_PIXELS / 16.0, 0.0]);
        assert_eq!(butt[0] < 7.0, visible, "the fixture's butt is at {butt:?}");
        assert_eq!(
            super::super::rig_may_be_visible(&rig, actor, 0.5, Some(view), None, |_, _| false),
            visible,
            "trident at x = {x}"
        );
    }
}

#[test]
fn a_thrown_trident_is_lit_at_its_own_position_in_any_rotation() {
    for (yaw, pitch) in [(0.0, -90.0), (0.0, 90.0), (-135.0, 30.0)] {
        let world = world(yaw, pitch);
        let rig = world.actor_rig(1).unwrap();
        let actor = world.actor(1).unwrap().clone();
        let rows = sampled_rig_placement(&rig, &actor, 0.5, [1.0; 4])
            .unwrap()
            .world_from_model;
        let position = Vec3::from_array(ModelFrame::of(&actor).actor_position(&rows));
        assert!(
            position.abs_diff_eq(Vec3::from_array(POSITION), 1e-5),
            "yaw {yaw} pitch {pitch}: lit at {position}"
        );
    }
    let mut arrow = world(0.0, 0.0).actor(1).unwrap().clone();
    arrow.entity_fact = assets::facts::entity::EntityFacts::pinned().get("minecraft:arrow");
    assert_eq!(ModelFrame::of(&arrow), ModelFrame::Upright);
    assert_eq!(
        ModelFrame::Upright
            .actor_position(&super::super::rig_world_from_actor(POSITION, 30.0, 2.0)),
        POSITION
    );
}

#[test]
fn a_turned_thrown_trident_stays_inside_its_culling_bounds() {
    let world = world(90.0, 0.0);
    let rig: ActorRigSnapshot<'_> = world.actor_rig(1).unwrap();
    let actor = world.actor(1).unwrap().clone();
    let presentation =
        super::super::entity_rig_presentation(&rig, &actor, &ActorArtworkPages::default(), 0.5)
            .expect("the thrown trident presents");
    let rows = presentation.submission.world_from_actor;
    let bounds = presentation.submission.culling_bounds;
    let origin = Vec3::from_array(transform(&rows, [0.0; 3]));
    for pixels in [HEAD_PIXELS, BUTT_PIXELS] {
        let offset = Vec3::from_array(transform(&rows, [0.0, pixels / 16.0, 0.0])) - origin;
        let low = Vec3::from_array(bounds.center) - Vec3::from_array(bounds.half_extents);
        let high = Vec3::from_array(bounds.center) + Vec3::from_array(bounds.half_extents);
        assert!(
            offset.cmpge(low).all() && offset.cmple(high).all(),
            "{offset}"
        );
    }
}

#[test]
fn only_an_enchanted_thrown_trident_gets_the_item_foil() {
    let world = world(0.0, 0.0);
    let mut actor = world.actor(1).unwrap().clone();
    let foil = |actor: &ActorSnapshot| {
        let mut presentation =
            local_presentation(super::super::rig_world_from_actor(POSITION, 0.0, 1.0)).unwrap();
        present_thrown_trident(&mut presentation.submission, actor);
        presentation.submission.material.glint
    };
    assert_eq!(foil(&actor), render_model::ActorGlint::default());
    actor.metadata.insert(
        protocol_ids::ActorDataKey::Flags.wire_value(),
        protocol::ActorMetadataValue::Flags(1 << protocol_ids::ActorFlag::Enchanted.wire_value()),
    );
    let glint = foil(&actor);
    assert_eq!(glint.texture, render_model::GlintTexture::Item);
    assert_eq!(glint.uv_scale, [FOIL_UV_SCALE; 2]);
    actor.entity_fact = assets::facts::entity::EntityFacts::pinned().get("minecraft:arrow");
    assert_eq!(foil(&actor), render_model::ActorGlint::default());
}

/// A placed diagnostic submission carrying `rows`, standing in for the rig's own submission.
fn local_presentation(rows: [[f32; 4]; 3]) -> Option<super::super::ActorRigPresentation> {
    let mut presentation = super::super::local_diagnostic_presentation(
        7,
        0,
        7,
        render_api::PoseGeneration::new(5),
        [0.0; 3],
        0.0,
        0.0,
    )?;
    presentation.submission.world_from_actor = rows;
    Some(presentation)
}

/// A camera at (0, 2, 10) looking down -Z with a 90 degree field of view, so seven blocks
/// ahead at z = 3 the right frustum edge is at x = 7.
fn side_view() -> render_model::ActorCullView {
    let camera = Vec3::new(0.0, 2.0, 10.0);
    render_model::ActorCullView {
        camera_position: camera,
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            90_f32.to_radians(),
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_to_mat4(camera, -Vec3::Z, Vec3::Y),
        max_distance: 100.0,
    }
}

#[test]
fn a_controller_selected_geometry_is_culled_in_the_trident_frame() {
    let view = Some(side_view());
    // Flying toward +X from x = 10, the selected six-block pole trails back to x = 4, into
    // view, while its upright box at the model origin (x = 8.5) stays right of the edge.
    let world = world_with([10.0, 2.0, 3.0], 90.0, 0.0, true);
    let rig = world.actor_rig(1).unwrap();
    let actor = world.actor(1).unwrap();
    let mut frame = world.actor_render_frame(0.5);
    let geometry = frame.layers(1).unwrap()[0]
        .geometry
        .expect("the controller selects the long pole");
    let (assets, _) = rig.geometry_source().unwrap();
    let upright = assets.geometry_visible_bounds(geometry as usize).unwrap();
    let mut presentation =
        super::super::entity_rig_presentation(&rig, actor, &ActorArtworkPages::default(), 0.5)
            .unwrap();
    let rows = presentation.submission.world_from_actor;
    let butt = transform(&rows, [0.0, -72.0 / 16.0, 0.0]);
    assert!(butt[0] < 7.0, "the selected pole's butt is at {butt:?}");
    let origin = rows.map(|row| row[3]);
    assert!(!render_model::actor_bounds_are_visible(
        origin, 1.0, upright, view
    ));
    let (_, locations) =
        ActorArtworkPages::default().with_equipment_rasters(&[render_model::EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([255; 4]),
        }]);
    presentation.artwork = locations[0];
    presentation.submission.route = render_model::ActorRigRoute::Compiled;
    assert!(
        !render_model::actor_rig_submission_is_visible(&presentation.submission, view),
        "the base pole alone is out of view"
    );
    assert!(super::super::admit_frame_bounds(
        &mut presentation,
        &rig,
        view,
        Some(&mut frame)
    ));
    assert!(render_model::actor_rig_submission_is_visible(
        &presentation.submission,
        view
    ));
}

#[test]
fn unlocated_trident_effects_start_at_its_position_and_located_ones_follow_the_model() {
    for (yaw, pitch) in [(0.0, 0.0), (-135.0, 30.0)] {
        let world = world(yaw, pitch);
        let rig = world.actor_rig(1).unwrap();
        let actor = world.actor(1).unwrap();
        let rows = sampled_rig_placement(&rig, actor, 0.5, [1.0; 4])
            .unwrap()
            .world_from_model;
        for locator in [None, Some("missing")] {
            let emitter =
                super::super::actor_emitter_transform(actor, Some(&rig), &[], locator, 0.5)
                    .unwrap();
            assert!(
                Vec3::from_array(emitter.position).abs_diff_eq(Vec3::from_array(POSITION), 1e-5),
                "yaw {yaw} pitch {pitch} locator {locator:?}: emitter at {:?}",
                emitter.position
            );
            assert_eq!(
                emitter.basis,
                std::array::from_fn(|axis| rows.map(|row| row[axis]))
            );
        }
    }
}
