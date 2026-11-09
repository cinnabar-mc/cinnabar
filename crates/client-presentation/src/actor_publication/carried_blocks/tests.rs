use std::sync::Arc;

use assets::{RuntimeAssets, RuntimeEntityAssets};
use glam::{Quat, Vec3};
use protocol::{
    ActorEvent, ActorMoveEvent, ActorPositionOrigin, ActorSpawnEvent, WorldBootstrap, WorldEvent,
};
use render_model::{ActorArtworkPages, ActorRigSubmission};

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
/// Plays the pack's pose and nothing else, so an ordinary rig keeps its tick endpoints.
const GOLEM_SCRIPTS: &str = r#""animate":["pose"]"#;
/// A model shaped like the vanilla golem, with the head under a body piece.
const GOLEM_GEOMETRY: &[u8] = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.golem","texture_width":64,"texture_height":64},"bones":[{"name":"piece2","pivot":[0,0,0]},{"name":"piece1","parent":"piece2","pivot":[0,11,0]},{"name":"head","parent":"piece1","pivot":[0,20,0],"cubes":[{"origin":[-4,20,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#;

/// A stream holding one spawned snow golem on the vanilla-shaped model, whose pack authors `bones`
/// as its animation.
fn golem_stream(bones: &str, pitch: f32, head_yaw: f32) -> WorldStream {
    golem_stream_on(GOLEM_GEOMETRY, GOLEM_SCRIPTS, bones, pitch, head_yaw)
}

/// A stream holding one spawned snow golem drawn with `geometry` and running `scripts`.
fn golem_stream_on(
    geometry: &[u8],
    scripts: &str,
    bones: &str,
    pitch: f32,
    head_yaw: f32,
) -> WorldStream {
    let identifier = assets::facts::entity::SNOW_GOLEM_IDENTIFIER;
    let definition = format!(
        r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"{identifier}","materials":{{"default":"entity"}},"textures":{{"default":"textures/entity/test"}},"geometry":{{"default":"geometry.golem"}},"animations":{{"pose":"animation.golem.pose"}},"scripts":{{{scripts}}},"render_controllers":["controller.render.golem"]}}}}}}"#
    );
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

/// The head bone's index in the golem rig's pose order.
fn head_index(stream: &WorldStream) -> usize {
    let authority = stream.authority();
    let rig = authority.actor_rig(GOLEM).expect("spawned golem rig");
    rig.bone_names
        .iter()
        .position(|name| name.as_ref() == "head")
        .expect("head bone")
}

/// The presented body and the pumpkin the frame at `alpha` places on it.
fn presented(stream: &WorldStream, alpha: f32) -> (ActorRigSubmission, [[f32; 4]; 3]) {
    let authority = stream.authority();
    let rig = authority.actor_rig(GOLEM).expect("spawned golem rig");
    let actor = authority.actor(GOLEM).expect("spawned golem");
    let artwork = ActorArtworkPages::default();
    let body = entity_rig_presentation(&rig, actor, &artwork, alpha)
        .expect("evaluated golem")
        .submission;
    let batch = ActorPresentationBatch {
        submissions: vec![body.clone()],
        skin_layers: Vec::new(),
        artwork: HashMap::new(),
    };
    let mut frame = authority.actor_render_frame(alpha);
    let [pumpkin] = carried_blocks_for(&batch, stream, &mut frame, alpha)[..] else {
        panic!("a golem wears one pumpkin");
    };
    (body, pumpkin.world_from_block)
}

/// The placement of the pumpkin on a golem whose pack holds `bones`, in the world.
fn pumpkin_with(bones: &str) -> [[f32; 4]; 3] {
    presented(&golem_stream(bones, 0.0, 0.0), 1.0).1
}

/// A placement's up axis in the rig frame of `body`.
fn up_in_rig(body: &ActorRigSubmission, world_from_block: [[f32; 4]; 3]) -> Vec3 {
    let up = Vec3::from(world_from_block.map(|row| row[1]));
    let rig_axis =
        |index: usize| Vec3::from(body.world_from_actor.map(|row| row[index])).normalize();
    Vec3::new(
        up.dot(rig_axis(0)),
        up.dot(rig_axis(1)),
        up.dot(rig_axis(2)),
    )
    .normalize()
}

/// The up axis of the pumpkin on a golem whose pack holds `bones`, in its rig frame.
fn pumpkin_up_with(bones: &str) -> Vec3 {
    let (body, pumpkin) = presented(&golem_stream(bones, 0.0, 0.0), 1.0);
    up_in_rig(&body, pumpkin)
}

/// The up axis a head authored at `pitch` and `yaw` degrees turns to in the mirrored rig frame.
fn authored_up(pitch: f32, yaw: f32) -> Vec3 {
    Quat::from_rotation_y(-yaw.to_radians()) * Quat::from_rotation_x(-pitch.to_radians()) * Vec3::Y
}

fn same_placement(a: [[f32; 4]; 3], b: [[f32; 4]; 3]) -> bool {
    a.iter()
        .flatten()
        .zip(b.iter().flatten())
        .all(|(a, b)| (a - b).abs() < 1e-4)
}

#[test]
fn a_snow_golem_pumpkin_keeps_the_rig_head_when_a_controller_draws_another_skeleton() {
    let stream = golem_stream(r#"{"head":{"rotation":[25,40,0]}}"#, 0.0, 0.0);
    let (mut body, same) = presented(&stream, 1.0);
    body.input.rig = render_model::layer_geometry_rig_id(body.input.rig, 1);
    body.input.previous_bones = Arc::from([]);
    body.input.current_bones = Arc::from([]);
    let batch = ActorPresentationBatch {
        submissions: vec![body],
        skin_layers: Vec::new(),
        artwork: HashMap::new(),
    };
    let mut frame = stream.authority().actor_render_frame(1.0);
    let [pumpkin] = carried_blocks_for(&batch, &stream, &mut frame, 1.0)[..] else {
        panic!("a golem wears one pumpkin");
    };
    assert_eq!(pumpkin.world_from_block, same);
}

#[test]
fn a_snow_golem_pumpkin_follows_a_head_the_pack_animates_while_its_actor_looks_ahead() {
    let stream = golem_stream(r#"{"head":{"rotation":[25,40,0]}}"#, 0.0, 0.0);
    let (body, pumpkin) = presented(&stream, 1.0);
    let head = Quat::from_array(body.input.current_bones[head_index(&stream)].rotation);
    let expected = authored_up(25.0, 40.0);
    assert!((head * Vec3::Y).distance(expected) < 1e-4, "{head}");
    let up = up_in_rig(&body, pumpkin);
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
    let (body, pumpkin) = presented(&stream, 1.0);
    let up = up_in_rig(&body, pumpkin);
    let expected = authored_up(20.0, 30.0);
    assert!(up.distance(expected) < 1e-4, "{up} != {expected}");
}

#[test]
fn a_snow_golem_pumpkin_ignores_a_parent_s_rotation_even_without_an_entity_relative_head() {
    let up = pumpkin_up_with(r#"{"piece1":{"rotation":[30,40,0]},"head":{"rotation":[10,0,0]}}"#);
    let expected = authored_up(10.0, 0.0);
    assert!(up.distance(expected) < 1e-4, "{up} != {expected}");
}

#[test]
fn a_roll_cannot_mix_into_a_sideways_head_s_pitch() {
    let rolled = pumpkin_with(r#"{"head":{"rotation":[20,90,30]}}"#);
    let plain = pumpkin_with(r#"{"head":{"rotation":[20,90,0]}}"#);
    assert!(same_placement(rolled, plain), "{rolled:?} != {plain:?}");
    // The rolled head composes to the same rotation as this one, which still pitches its own way.
    let lookalike = pumpkin_with(r#"{"head":{"rotation":[-10,90,0]}}"#);
    assert!(!same_placement(rolled, lookalike));
}

#[test]
fn a_sideways_head_keeps_its_pitch() {
    for (pitch, yaw) in [(20.0, 90.0), (20.0, -90.0), (-35.0, 90.0)] {
        let up = pumpkin_up_with(&format!(r#"{{"head":{{"rotation":[{pitch},{yaw},0]}}}}"#));
        let expected = authored_up(pitch, yaw);
        assert!(
            up.distance(expected) < 1e-3,
            "{pitch} {yaw}: {up} != {expected}"
        );
    }
}

#[test]
fn a_pure_roll_past_a_quarter_turn_leaves_the_pumpkin_at_rest() {
    let rolled = pumpkin_with(r#"{"head":{"rotation":[0,0,120]}}"#);
    let rest = pumpkin_with(r#"{"head":{"rotation":[0,0,0]}}"#);
    assert!(same_placement(rolled, rest), "{rolled:?} != {rest:?}");
}

#[test]
fn a_yaw_past_a_quarter_turn_is_kept() {
    let up = pumpkin_up_with(r#"{"head":{"rotation":[20,120,0]}}"#);
    let expected = authored_up(20.0, 120.0);
    assert!(up.distance(expected) < 1e-4, "{up} != {expected}");
}

#[test]
fn a_snow_golem_pumpkin_interpolates_the_head_s_tick_history_like_the_pose() {
    let mut stream = golem_stream(
        r#"{"head":{"rotation":[20,"query.life_time * 200",0]}}"#,
        0.0,
        0.0,
    );
    let head = head_index(&stream);
    let check = |stream: &WorldStream| {
        for (alpha, pose) in [(0.0, true), (1.0, false)] {
            let (body, pumpkin) = presented(stream, alpha);
            let bones = if pose {
                &body.input.previous_bones
            } else {
                &body.input.current_bones
            };
            let expected = Quat::from_array(bones[head].rotation) * Vec3::Y;
            let up = up_in_rig(&body, pumpkin);
            assert!(up.distance(expected) < 1e-4, "{alpha}: {up} != {expected}");
        }
    };
    // Spawning starts both endpoints at one pose.
    check(&stream);
    stream.advance_actor_interpolation_ticks(1);
    let (moving, _) = presented(&stream, 1.0);
    assert_ne!(
        moving.input.previous_bones[head], moving.input.current_bones[head],
        "the clip must move between ticks"
    );
    check(&stream);
    stream.advance_actor_interpolation_ticks(1);
    check(&stream);
    // A culled tick holds the pose, and a rig back in view restarts from its new one.
    let view = actor_animation::ActorAnimationView {
        planes: [[0.0, 0.0, 0.0, -1.0]; 6],
        camera: [0.0; 3],
        player_distance: 100.0,
        entity_radius: 100.0,
    };
    stream.set_actor_animation_view(Some(view));
    stream.advance_actor_interpolation_ticks(1);
    let (held, _) = presented(&stream, 1.0);
    assert_eq!(
        held.input.previous_bones[head],
        held.input.current_bones[head]
    );
    check(&stream);
    stream.set_actor_animation_view(None);
    stream.advance_actor_interpolation_ticks(1);
    check(&stream);
}

#[test]
fn a_camera_sampled_head_moves_the_pumpkin_between_ticks() {
    let mut stream = golem_stream(
        r#"{"head":{"rotation":[20,"query.camera_rotation(1)",0]}}"#,
        0.0,
        0.0,
    );
    let (ticked, tick_pumpkin) = presented(&stream, 0.5);
    let tick_up = up_in_rig(&ticked, tick_pumpkin);
    assert!(tick_up.distance(authored_up(20.0, 0.0)) < 1e-4, "{tick_up}");
    // The camera turns without a tick, so only a frame sample can see it.
    stream.set_actor_camera_rotation([0.0, 70.0]);
    let expected = authored_up(20.0, 70.0);
    for _ in 0..2 {
        // The second frame reuses the first frame's retained sample.
        let (body, pumpkin) = presented(&stream, 0.5);
        let up = up_in_rig(&body, pumpkin);
        assert!(up.distance(expected) < 1e-4, "{up} != {expected}");
    }
}

#[test]
fn a_rotation_sampled_head_moves_the_pumpkin_at_its_frame_angle() {
    // A script reading the look makes the rig sample rotation per frame. Squaring the look makes
    // the frame's angle differ from a blend of the two tick poses.
    let mut stream = golem_stream_on(
        GOLEM_GEOMETRY,
        r#""pre_animation":["variable.look = query.target_y_rotation;"],"animate":["pose"]"#,
        r#"{"head":{"rotation":[20,"math.pow(query.target_y_rotation, 2) / 40",0]}}"#,
        0.0,
        0.0,
    );
    stream.advance_actor_interpolation_ticks(1);
    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: GOLEM,
                position: [None; 3],
                position_origin: ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: Some(40.0),
                on_ground: Some(true),
                teleported: false,
                force_local_move: false,
                player_mode: None,
                source_tick: None,
                interpolation: Default::default(),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(1);
    let head_at = |alpha| {
        stream
            .authority()
            .actor_render_frame(alpha)
            .head_rotation(GOLEM)
            .expect("a golem's head angles")
    };
    // Each end of the tick pair reads one look, whose square root is the look's size.
    let [start, end] = [head_at(0.0), head_at(1.0)].map(|head| head.current[1]);
    let look = |yaw: f32| (yaw * 40.0).sqrt();
    let frame_yaw = ((look(start) + look(end)) * 0.5).powi(2) / 40.0;
    let blended_yaw = (start + end) * 0.5;
    assert!(
        (frame_yaw - blended_yaw).abs() > 0.1,
        "the frame must not read as a tick blend: {start} {end}"
    );
    let frame = head_at(0.5);
    assert_eq!(
        frame.previous, frame.current,
        "both endpoints read the frame's look"
    );
    assert!(
        (frame.current[1] - frame_yaw).abs() < 1e-3,
        "{frame:?} != {frame_yaw}"
    );
    let (body, pumpkin) = presented(&stream, 0.5);
    let up = up_in_rig(&body, pumpkin);
    let expected = authored_up(20.0, frame_yaw);
    assert!(up.distance(expected) < 1e-3, "{up} != {expected}");
}

#[test]
fn a_golem_whose_rig_has_no_head_part_wears_nothing() {
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.golem","texture_width":64,"texture_height":64},"bones":[{"name":"body","pivot":[0,0,0],"cubes":[{"origin":[-4,0,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#;
    let stream = golem_stream_on(
        geometry,
        GOLEM_SCRIPTS,
        r#"{"body":{"rotation":[0,0,0]}}"#,
        0.0,
        0.0,
    );
    let authority = stream.authority();
    let rig = authority.actor_rig(GOLEM).expect("spawned golem rig");
    let actor = authority.actor(GOLEM).expect("spawned golem");
    let body = entity_rig_presentation(&rig, actor, &ActorArtworkPages::default(), 1.0)
        .expect("evaluated golem")
        .submission;
    let batch = ActorPresentationBatch {
        submissions: vec![body],
        skin_layers: Vec::new(),
        artwork: HashMap::new(),
    };
    let mut frame = authority.actor_render_frame(1.0);
    assert!(carried_blocks_for(&batch, &stream, &mut frame, 1.0).is_empty());
}
