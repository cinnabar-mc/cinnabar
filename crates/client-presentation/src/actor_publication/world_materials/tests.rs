use super::*;
use render_scene::world_material::WorldMaterialScene;

#[test]
fn bound_entities_tag_body_and_controller_layers_only() {
    let scene = WorldMaterialScene::with_bound_entities(&["example:other", "example:moth"]);
    let moth = protocol::ActorKind::Entity {
        identifier: "example:moth".into(),
    };
    assert_eq!(world(render_model::ACTOR_LAYER_BODY, &moth, &scene), 2);
    assert_eq!(world(ACTOR_LAYER_TEXTURE_BASE, &moth, &scene), 2);
    assert_eq!(
        world(render_model::ACTOR_LAYER_BODY + 1, &moth, &scene),
        0,
        "equipment keeps its own material"
    );
    let unbound = protocol::ActorKind::Entity {
        identifier: "example:pig".into(),
    };
    assert_eq!(world(render_model::ACTOR_LAYER_BODY, &unbound, &scene), 0);
    let player = protocol::ActorKind::Player {
        uuid: [0; 16],
        username: "example:moth".into(),
    };
    assert_eq!(world(render_model::ACTOR_LAYER_BODY, &player, &scene), 0);
}
