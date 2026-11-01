use std::sync::Arc;

use assets::{RuntimeAssets, RuntimeEntityAssets};
use glam::{Quat, Vec3};
use protocol::{ActorEvent, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render_model::ActorArtworkPages;

use super::*;
use crate::presentation::actors::entity_rig_presentation;

fn entity(identifier: &str) -> ActorKind {
    ActorKind::Entity {
        identifier: identifier.into(),
    }
}

fn carrying(id: i32) -> HashMap<u32, ActorMetadataValue> {
    HashMap::from([(
        ActorDataKey::CarryBlockRuntimeID.wire_value(),
        ActorMetadataValue::Int(id),
    )])
}

#[test]
fn an_enderman_draws_its_synced_carried_block() {
    let enderman = entity(assets::facts::entity::ENDERMAN_IDENTIFIER);
    assert_eq!(carried_block_runtime_id(&enderman, &carrying(42)), Some(42));
    // Hashed palettes sync ids above i32::MAX as negative ints.
    assert_eq!(
        carried_block_runtime_id(&enderman, &carrying(-2)),
        Some(u32::MAX - 1)
    );
    assert_eq!(carried_block_runtime_id(&enderman, &HashMap::new()), None);
}

#[test]
fn only_the_enderman_renderer_draws_a_carried_block() {
    assert_eq!(
        carried_block_runtime_id(&entity("minecraft:zombie"), &carrying(42)),
        None
    );
}

#[test]
fn an_unsolved_carrier_does_not_darken_its_block() {
    assert_eq!(carried_light(Some((4, 12))), (4, 12));
    assert_eq!(carried_light(Some((0, 0))), (0, 0));
    assert_eq!(carried_light(None), (15, 15));
}

#[test]
fn snow_golems_wear_a_pumpkin_until_sheared() {
    let golem = entity(assets::facts::entity::SNOW_GOLEM_IDENTIFIER);
    assert!(wears_pumpkin(&golem, false));
    assert!(!wears_pumpkin(&golem, true));
    assert!(!wears_pumpkin(
        &entity(assets::facts::entity::ENDERMAN_IDENTIFIER),
        false
    ));
    // The golem renderer ignores the synced carried block, which only endermen draw.
    assert_eq!(carried_block_runtime_id(&golem, &carrying(42)), None);
}

#[test]
fn a_snow_golem_pumpkin_is_a_pinned_carved_pumpkin_state() {
    let state = snow_golem_pumpkin().expect("pinned carved pumpkin");
    let (name, canonical) = assets::pinned_block_identity(state).expect("pinned identity");
    assert_eq!(
        name,
        assets::facts::pack::PackBlock::CarvedPumpkin.identifier()
    );
    assert!(canonical.contains(BlockProperty::MinecraftCardinalDirection.name()));
}

const GOLEM: u64 = 5;

/// A stream holding one spawned snow golem whose model is shaped like the vanilla one, with the
/// head under a body piece, and whose pack authors `bones` as its animation.
fn golem_stream(bones: &str, pitch: f32, head_yaw: f32) -> WorldStream {
    let identifier = assets::facts::entity::SNOW_GOLEM_IDENTIFIER;
    let definition = format!(
        r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"{identifier}","materials":{{"default":"entity"}},"textures":{{"default":"textures/entity/test"}},"geometry":{{"default":"geometry.golem"}},"animations":{{"pose":"animation.golem.pose"}},"scripts":{{"animate":["pose"]}},"render_controllers":["controller.render.golem"]}}}}}}"#
    );
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.golem","texture_width":64,"texture_height":64},"bones":[{"name":"piece2","pivot":[0,0,0]},{"name":"piece1","parent":"piece2","pivot":[0,11,0]},{"name":"head","parent":"piece1","pivot":[0,20,0],"cubes":[{"origin":[-4,20,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#;
    let animation = format!(
        r#"{{"format_version":"1.8.0","animations":{{"animation.golem.pose":{{"loop":true,"bones":{bones}}}}}}}"#
    );
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.golem":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let compiled = pack_entity_compiler::compile_entity_pack(vec![
        ("entity/golem.json".into(), definition.into_bytes()),
        ("models/entity/golem.geo.json".into(), geometry.to_vec()),
        (
            "animations/golem.animation.json".into(),
            animation.into_bytes(),
        ),
        ("render_controllers/golem.json".into(), controller.to_vec()),
    ])
    .unwrap()
    .unwrap();
    let mut stream = WorldStream::new_with_asset_sets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        Arc::new(RuntimeEntityAssets::from_compiled(compiled.assets).unwrap()),
        [0.0, 64.0, 0.0],
        None,
    );
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: GOLEM as i64,
                runtime_id: GOLEM,
                kind: entity(identifier),
                position: [0.0, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch,
                yaw: 0.0,
                head_yaw,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_frame(1);
    stream
}

/// The drawn golem's head rotation and its pumpkin's up axis, both in the body's rig frame.
fn head_and_pumpkin_up(stream: &WorldStream) -> (Quat, Vec3) {
    let authority = stream.authority();
    let rig = authority.actor_rig(GOLEM).expect("spawned golem rig");
    let actor = authority.actor(GOLEM).expect("spawned golem");
    let artwork = ActorArtworkPages::default();
    let body = entity_rig_presentation(&rig, actor, &artwork, 1.0)
        .expect("evaluated golem")
        .submission;
    let head = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "head")
        .expect("head bone");
    let head = Quat::from_array(body.input.current_bones[head].rotation);
    let world_from_rig = body.world_from_actor;
    let batch = ActorPresentationBatch {
        submissions: vec![body],
        skin_layers: Vec::new(),
        artwork: HashMap::new(),
    };
    let [pumpkin] = carried_blocks_for(&batch, stream, 1.0)[..] else {
        panic!("a golem wears one pumpkin");
    };
    let up = Vec3::from(pumpkin.world_from_block.map(|row| row[1]));
    let rig_axis = |index: usize| Vec3::from(world_from_rig.map(|row| row[index])).normalize();
    let up = Vec3::new(
        up.dot(rig_axis(0)),
        up.dot(rig_axis(1)),
        up.dot(rig_axis(2)),
    );
    (head, up.normalize())
}

/// The up axis a head authored at `pitch` and `yaw` degrees turns to in the mirrored rig frame.
fn authored_up(pitch: f32, yaw: f32) -> Vec3 {
    Quat::from_rotation_y(-yaw.to_radians()) * Quat::from_rotation_x(-pitch.to_radians()) * Vec3::Y
}

#[test]
fn a_snow_golem_pumpkin_keeps_the_rig_head_when_a_controller_draws_another_skeleton() {
    let stream = golem_stream(r#"{"head":{"rotation":[25,40,0]}}"#, 0.0, 0.0);
    let authority = stream.authority();
    let rig = authority.actor_rig(GOLEM).expect("spawned golem rig");
    let actor = authority.actor(GOLEM).expect("spawned golem");
    let artwork = ActorArtworkPages::default();
    let mut body = entity_rig_presentation(&rig, actor, &artwork, 1.0)
        .expect("evaluated golem")
        .submission;
    let same = {
        let batch = ActorPresentationBatch {
            submissions: vec![body.clone()],
            skin_layers: Vec::new(),
            artwork: HashMap::new(),
        };
        let [pumpkin] = carried_blocks_for(&batch, &stream, 1.0)[..] else {
            panic!("a golem wears one pumpkin");
        };
        pumpkin.world_from_block
    };
    body.input.rig = render_model::layer_geometry_rig_id(body.input.rig, 1);
    body.input.previous_bones = Arc::from([]);
    body.input.current_bones = Arc::from([]);
    let batch = ActorPresentationBatch {
        submissions: vec![body],
        skin_layers: Vec::new(),
        artwork: HashMap::new(),
    };
    let [pumpkin] = carried_blocks_for(&batch, &stream, 1.0)[..] else {
        panic!("a golem wears one pumpkin");
    };
    assert_eq!(pumpkin.world_from_block, same);
}

#[test]
fn a_snow_golem_pumpkin_follows_a_head_the_pack_animates_while_its_actor_looks_ahead() {
    let stream = golem_stream(r#"{"head":{"rotation":[25,40,0]}}"#, 0.0, 0.0);
    let (head, up) = head_and_pumpkin_up(&stream);
    let expected = authored_up(25.0, 40.0);
    assert!((head * Vec3::Y).distance(expected) < 1e-4, "{head}");
    assert!(up.distance(expected) < 1e-4, "{up} != {expected}");
}

#[test]
fn a_snow_golem_pumpkin_keeps_following_the_vanilla_look_animation() {
    // The vanilla pack turns the body piece by a quarter of the look, under a head posed relative
    // to the entity, so the piece's turn must not reach the pumpkin.
    let stream = golem_stream(
        r#"{"piece1":{"rotation":[0,"query.target_y_rotation * 0.25",0]},
            "head":{"relative_to":{"rotation":"entity"},
                    "rotation":["query.target_x_rotation - this","query.target_y_rotation - this",0]}}"#,
        20.0,
        30.0,
    );
    let (_, up) = head_and_pumpkin_up(&stream);
    let expected = authored_up(20.0, 30.0);
    assert!(up.distance(expected) < 1e-4, "{up} != {expected}");
}
