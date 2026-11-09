use server_experience::{
    policy::MAX_TRIANGLES,
    runtime::{Contributions, SceneObject},
};

#[test]
fn asynchronous_allowance_includes_legacy_objects_and_existing_drawing() {
    let mut scene = Contributions::default();
    scene.scene.insert(
        1,
        SceneObject::Quad {
            texture: "fixture".into(),
            transform: [0., 0., 0., 0., 0., 0., 1., 1., 1., 1.],
            size: [1.; 2],
        },
    );
    scene.scene.insert(
        2,
        SceneObject::Mesh {
            asset: "fixture".into(),
            transform: [0., 0., 0., 0., 0., 0., 1., 1., 1., 1.],
            triangles: MAX_TRIANGLES - 10,
        },
    );
    assert_eq!(scene.remaining_scene_triangles(1, 3).unwrap(), 5);
    assert!(scene.remaining_scene_triangles(1, 9).is_err());
}
