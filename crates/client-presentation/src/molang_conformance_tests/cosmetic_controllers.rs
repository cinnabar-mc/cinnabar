//! A player pack that lists conditional cosmetic controllers before the body controller.
use super::*;
use actor_animation::{ActorAnimationStore, ActorAnimationView, ActorTickContext};
use std::collections::HashMap;

/// An original player pack: a hat controller picks its model from an array by
/// `query.variant`, ahead of the plain body controller, as cosmetic server packs do.
fn cosmetic_pack() -> Pack {
    let entity = serde_json::json!({"format_version":"1.10.0","minecraft:client_entity":{
    "description":{
        "identifier":"minecraft:player",
        "materials":{"default":"entity_alphatest"},
        "textures":{"default":"textures/entity/player","hat":"textures/entity/player"},
        "geometry":{"default":"geometry.player","hat":"geometry.hat"},
        "scripts":{"animate":["swing"]},
        "animations":{"swing":"animation.player.swing"},
        "render_controllers":[
            {"controller.render.hat":"!variable.is_first_person"},
            {"controller.render.player":"!variable.is_first_person"}
        ]}}});
    let cube = |origin: [i32; 3], size: [i32; 3]| serde_json::json!([{"origin": origin, "size": size, "uv": [0, 0]}]);
    // A narrow body and a hat whose authored visible box reaches far to either side.
    let bounded = |mut geometry: serde_json::Value, width: u32| {
        let description = &mut geometry["minecraft:geometry"][0]["description"];
        description["visible_bounds_width"] = width.into();
        description["visible_bounds_height"] = 2.into();
        description["visible_bounds_offset"] = serde_json::json!([0, 1, 0]);
        geometry
    };
    write_pack(
        "player",
        vec![
            ("entity/player.entity.json", entity),
            (
                "models/entity/player.geo.json",
                bounded(
                    geometry_file(
                        "player",
                        vec![
                            serde_json::json!({"name":"body","pivot":[0,24,0],
                            "cubes":cube([-4, 12, -2], [8, 12, 4])}),
                            serde_json::json!({"name":"head","parent":"body","pivot":[0,24,0],
                            "cubes":cube([-4, 24, -4], [8, 8, 8])}),
                            serde_json::json!({"name":"leftLeg","parent":"body","pivot":[2,12,0],
                            "cubes":cube([0, 0, -2], [4, 12, 4])}),
                        ],
                    ),
                    1,
                ),
            ),
            (
                "models/entity/hat.geo.json",
                bounded(
                    geometry_file(
                        "hat",
                        vec![
                            serde_json::json!({"name":"body","pivot":[0,24,0]}),
                            serde_json::json!({"name":"head","parent":"body","pivot":[0,24,0]}),
                            serde_json::json!({"name":"hat","parent":"head","pivot":[0,32,0],
                            "cubes":cube([-3, 32, -3], [6, 3, 6])}),
                        ],
                    ),
                    40,
                ),
            ),
            (
                "animations/player.animation.json",
                serde_json::json!({"format_version":"1.8.0","animations":{
                    "animation.player.swing":{"loop":true,
                        "bones":{"leftLeg":{"rotation":[30.0,0.0,0.0]},
                            "head":{"rotation":[0.0,20.0,0.0]}}}}}),
            ),
            (
                "render_controllers/player.render_controllers.json",
                serde_json::json!({"format_version":"1.8.0","render_controllers":{
                    "controller.render.hat":{
                        "arrays":{
                            "geometries":{"Array.geos":["Geometry.default","Geometry.hat"]},
                            "textures":{"Array.skins":["Texture.default","Texture.hat"]}},
                        "geometry":"Array.geos[math.mod(q.variant + 1, 2)]",
                        "materials":[{"*":"Material.default"}],
                        "textures":["Array.skins[math.mod(q.variant + 1, 2)]"]},
                    "controller.render.player":{"geometry":"Geometry.default",
                        "materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}),
            ),
        ],
    )
}

/// Compiles the pack and advances a player through two ticks.
fn tick_player(pack: &Pack) -> (ActorAnimationStore, Arc<RuntimeEntityAssets>) {
    tick_player_in(pack, None)
}

/// Advances the player through two ticks, culling its animation against `view`.
fn tick_player_in(
    pack: &Pack,
    view: Option<&ActorAnimationView>,
) -> (ActorAnimationStore, Arc<RuntimeEntityAssets>) {
    let (world, entities) = spawned("player", pack);
    let mut actor = world.authority().actor(5).unwrap().clone();
    actor.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "fixture".into(),
    };
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&entities));
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(5, actor)]);
    for _ in 0..2 {
        store.advance_tick(&actors, view, None, true, false, |_| {
            ActorTickContext::default()
        });
    }
    (store, entities)
}

/// Authored X angle of a bone from its quaternion, undoing the rig frame's mirrored X rotation.
fn x_angle(transform: &actor_animation::BoneTransform) -> f32 {
    -2.0 * transform.rotation[0]
        .atan2(transform.rotation[3])
        .to_degrees()
}

#[test]
fn a_cosmetic_controller_ahead_of_the_body_does_not_replace_the_players_model() {
    let pack = cosmetic_pack();
    let (store, entities) = tick_player(&pack);
    let rig = store.get(5).unwrap();
    assert!(rig.pose_evaluated);
    let geometry =
        &entities.geometries()[entities.rig_geometries()[rig.rig.0 as usize].geometry as usize];
    assert_eq!(geometry.identifier.as_ref(), "geometry.player");
    let leg = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "leftleg")
        .expect("the body keeps its legs");
    assert!(
        (x_angle(&rig.current[leg]) - 30.0).abs() < 1.0e-3,
        "the player's own animation drives its body"
    );
}

#[test]
fn every_passing_player_controller_draws_its_own_model() {
    let pack = cosmetic_pack();
    let (store, entities) = tick_player(&pack);
    let rig = store.get(5).unwrap();
    let hat = entities
        .geometries()
        .iter()
        .position(|geometry| geometry.identifier.as_ref() == "geometry.hat")
        .unwrap() as u32;
    let cosmetic = rig
        .render
        .iter()
        .find(|layer| layer.geometry == Some(hat))
        .expect("the hat controller draws its model");
    assert!(
        cosmetic
            .pose
            .iter()
            .any(|bone| bone.rotation != [0.0, 0.0, 0.0, 1.0]),
        "the hat model follows the player's animated head"
    );
    assert!(
        rig.render.iter().any(|layer| layer.geometry.is_none()),
        "the body controller still draws the player's model"
    );
}

#[test]
fn a_player_whose_controller_model_reaches_into_view_still_animates() {
    let pack = cosmetic_pack();
    // The view keeps `x <= -2` from a camera behind both boxes: the narrow body at the origin
    // is outside, the wide hat inside.
    let view = ActorAnimationView {
        planes: [[-1.0, 0.0, 0.0, -2.0]; 6],
        camera: [0.0, 64.0, 30.0],
        player_distance: 100.0,
        entity_radius: 72.0,
    };
    let (store, _) = tick_player_in(&pack, Some(&view));
    assert!(
        store.get(5).unwrap().pose_evaluated,
        "the hat's visible box keeps the player animating"
    );
}
