//! Original thrown-trident fixtures for the native flight placement and foil.

use super::super::sampled_rig_placement;
use super::*;
use actor_animation::ActorRigSnapshot;
use image::ImageEncoder;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use std::sync::Arc;

const POSITION: [f32; 3] = [1.0, 2.0, 3.0];
/// Authored heights of the fixture's head and butt; the native origin is at 24 pixels.
const HEAD_PIXELS: f32 = 28.0;
const BUTT_PIXELS: f32 = -3.0;

/// Compiles an original pole model, with its pivot at the native origin, without game assets.
fn world(yaw: f32, pitch: f32) -> client_world::WorldAuthority {
    let identifier = NativeEntityClass::ThrownTrident.identifier().unwrap();
    let entity = format!(
        r#"{{"format_version":"1.8.0","minecraft:client_entity":{{"description":{{
        "identifier":"{identifier}","textures":{{"default":"textures/entity/pole"}},
        "geometry":{{"default":"geometry.pole"}}}}}}}}"#
    );
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{
        "identifier":"geometry.pole","texture_width":4,"texture_height":32},
        "bones":[{"name":"pole","pivot":[0,24,0],"cubes":[{"origin":[-0.5,-3,-0.5],
        "size":[1,31,1],"uv":[0,0]}]}]}]}"#;
    let mut texture = Vec::new();
    image::codecs::png::PngEncoder::new(&mut texture)
        .write_image(&[40, 120, 110, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let compiled = pack_entity_compiler::compile_entity_pack(vec![
        ("entity/pole.json".into(), entity.into_bytes()),
        ("models/entity/pole.geo.json".into(), geometry.to_vec()),
        ("textures/entity/pole.png".into(), texture),
    ])
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
                position: POSITION,
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
        let (rows, _) = sampled_rig_placement(&rig, &actor, 0.5, [1.0; 4]).unwrap();
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
fn a_thrown_trident_is_lit_at_its_own_position_in_any_rotation() {
    for (yaw, pitch) in [(0.0, -90.0), (0.0, 90.0), (-135.0, 30.0)] {
        let world = world(yaw, pitch);
        let rig = world.actor_rig(1).unwrap();
        let actor = world.actor(1).unwrap().clone();
        let (rows, _) = sampled_rig_placement(&rig, &actor, 0.5, [1.0; 4]).unwrap();
        let position = Vec3::from_array(thrown_trident_position(&actor, &rows).unwrap());
        assert!(
            position.abs_diff_eq(Vec3::from_array(POSITION), 1e-5),
            "yaw {yaw} pitch {pitch}: lit at {position}"
        );
    }
    let mut arrow = world(0.0, 0.0).actor(1).unwrap().clone();
    arrow.entity_fact = assets::facts::entity::EntityFacts::pinned().get("minecraft:arrow");
    assert_eq!(
        thrown_trident_position(
            &arrow,
            &super::super::rig_world_from_actor(POSITION, 0.0, 1.0)
        ),
        None
    );
}

#[test]
fn a_turned_thrown_trident_stays_inside_its_culling_bounds() {
    let world = world(90.0, 0.0);
    let rig: ActorRigSnapshot<'_> = world.actor_rig(1).unwrap();
    let actor = world.actor(1).unwrap().clone();
    let (rows, _) = sampled_rig_placement(&rig, &actor, 0.5, [1.0; 4]).unwrap();
    let mut presentation =
        local_presentation(rows).expect("a fixture submission to place the trident in");
    present_thrown_trident(&mut presentation.submission, &actor);
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
