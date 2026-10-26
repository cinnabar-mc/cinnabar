use super::*;
use mod_api::drawing::DrawingPermissions;
use mod_render::retained::{self, GeometrySpec, Instance, Store};

/// Creates a grid and a live placement using the real retained-resource contract.
fn grid_scene() -> (Store, DrawingHandle, DrawingHandle, Instance) {
    let mut store = Store::default();
    let grid = store
        .create_geometry(GeometrySpec::Blocks(retained::blocks::BlockGrid {
            revision: 0,
            size: [1; 3],
            palette: vec![retained::blocks::BlockState::RuntimeId(0)],
            cells: vec![retained::blocks::BlockCell {
                palette: 0,
                sky_light: None,
                block_light: None,
            }],
        }))
        .unwrap();
    let instance = Instance {
        geometry: grid,
        material: None,
        clip: None,
        params: mod_api::materials::Preset::default().0,
        anchor: retained::Anchor::Camera,
        transform: default(),
        tint: [1.; 4],
        depth: default(),
        blend: default(),
        width: 1.,
        width_mode: default(),
        join: default(),
        cap: default(),
    };
    let id = store
        .create(instance.clone(), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    (store, grid, id, instance)
}

#[test]
fn portal_resources_release_a_block_placement_after_an_instance_changes_geometry() {
    let (mut store, grid, id, mut instance) = grid_scene();
    let original = Arc::clone(&store.snapshot().geometry[&grid]);
    let live = BTreeMap::from([(grid, Arc::clone(&original))]);
    assert!(prepare::placement_is_current(
        &[Arc::clone(store.snapshot())],
        &live,
        id,
        &original
    ));
    let triangle = store
        .create_geometry(GeometrySpec::Triangles {
            vertices: [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
                .map(|position| retained::Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        })
        .unwrap();
    instance.geometry = triangle;
    store.update(id, instance, DrawingPermissions::ALL).unwrap();
    store.remove(grid).unwrap();
    store.commit();
    assert!(!prepare::placement_is_current(
        &[Arc::clone(store.snapshot())],
        &BTreeMap::new(),
        id,
        &original
    ));
}

#[test]
fn portal_resources_preserve_outward_faces_under_a_mirrored_transform() {
    let key = Key {
        msaa: Msaa::Off,
        hdr: false,
        clipped: false,
        always: false,
        additive: false,
        mirrored: true,
    };
    for mut descriptor in pipeline::layouts::descriptors::build(false) {
        let original = descriptor.primitive.front_face;
        Specialization.specialize(key, &mut descriptor).unwrap();
        assert_ne!(descriptor.primitive.front_face, original);
    }
}
