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

#[test]
fn portal_resources_enforce_combined_bundle_triangles_after_worker_completion() {
    let (mut store, grid, _, instance) = grid_scene();
    let geometry = Arc::clone(&store.snapshot().geometry[&grid]);
    let block = geometry.blocks().unwrap();
    assert!(block.claim());
    block.complete(Ok(BlockMesh {
        sections: Vec::new(),
        triangles: 12,
        bytes: 0,
    }));
    store.create(instance, DrawingPermissions::ALL).unwrap();
    store.commit();
    let mut scene = ModRenderScene::default();
    scene.retained.push(Arc::clone(store.snapshot()));
    scene.gpu_reservations.push(mod_render::GpuReservation {
        owners: vec![grid.owner],
        ledger: mod_render::GpuLedger::new(1024),
        primitives: default(),
        passes: Vec::new(),
        retained_triangle_limit: Some(20),
    });
    prepare::admit_triangles(&scene);
    assert_eq!(
        block.render_error().map(|error| error.code),
        Some(retained::ErrorCode::BudgetExceeded)
    );
}

#[test]
fn portal_resources_drain_completed_sorts_while_their_grid_is_offscreen() {
    let (mut app, _) = crate::queue_review_support::app();
    let (store, grid, id, _) = grid_scene();
    let device = app.world().resource::<RenderDevice>().clone();
    let queue = app.world().resource::<RenderQueue>().clone();
    let ledger = mod_render::GpuLedger::new(1024);
    let geometry = Arc::clone(&store.snapshot().geometry[&grid]);
    let native = Arc::new(Native {
        geometry,
        terrain: PreparedResourceGeometry::build(
            &[],
            ChunkTextureAssets::default(),
            device,
            queue,
            None,
        )
        .unwrap(),
        faces: Arc::from([]),
        _charge: ledger.allocate(0, || Some(())).unwrap(),
    });
    let uniform = {
        let world = app.world_mut();
        let mut query = world.query::<&ExtractedView>();
        bevy::render::render_resource::UniformBuffer::from(prepare::uniform(
            query.single(world).unwrap(),
            Mat4::IDENTITY,
            1.,
        ))
    };
    let pipeline = app
        .world()
        .resource::<PipelineCache>()
        .queue_render_pipeline(default());
    let pool = AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let job = pool.spawn(async { None });
    test_time::eventually("completed offscreen block sort", || job.is_finished());
    let key = (app.world_mut().spawn_empty().id(), id);
    let mut placements = BTreeMap::from([(
        key,
        Placement {
            uniform,
            matrix: Mat4::IDENTITY,
            projection: Mat4::IDENTITY,
            viewport: bevy::math::UVec4::ZERO,
            exposure: 1.,
            binding: None,
            opaque_binding: None,
            identity: None,
            native,
            order: None,
            order_job: Some(job),
            sort_camera: None,
            pipelines: [pipeline; 5],
            clip: None,
            active: false,
            _charge: ledger.allocate(0, || Some(())).unwrap(),
        },
    )]);
    prepare::poll_orders(&mut placements, &mut Vec::new());
    assert!(placements[&key].order_job.is_none());
}
