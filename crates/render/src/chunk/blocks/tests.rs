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

/// Supplies an admitted placement on the validated NOOP device without game carriers.
pub(super) fn placement_fixture() -> (
    App,
    (Entity, DrawingHandle),
    Placement,
    Arc<retained::Resources>,
) {
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
    let view = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, bevy::prelude::With<ExtractedView>>();
        query.single(world).unwrap()
    };
    let placement = Placement {
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
        order_job: None,
        sort_camera: None,
        pipelines: [pipeline; 5],
        clip: None,
        always: false,
        active: false,
        _charge: ledger.allocate(0, || Some(())).unwrap(),
    };
    (app, (view, id), placement, Arc::clone(store.snapshot()))
}

#[test]
fn portal_resources_drain_completed_sorts_while_their_grid_is_offscreen() {
    let (_, key, mut placement, _) = placement_fixture();
    let pool = AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let job = pool.spawn(async { None });
    test_time::eventually("completed offscreen block sort", || job.is_finished());
    placement.order_job = Some(job);
    let mut placements = BTreeMap::from([(key, placement)]);
    prepare::poll_orders(&mut placements, &mut Vec::new());
    assert!(placements[&key].order_job.is_none());
}

#[test]
fn retained_additive_terrain_weights_straight_alpha_only_for_blended_variants() {
    for clipped in [false, true] {
        for msaa in [Msaa::Off, Msaa::Sample4] {
            let key = Key {
                msaa,
                hdr: false,
                clipped,
                always: false,
                additive: true,
                mirrored: false,
            };
            for mut descriptor in pipeline::layouts::descriptors::build(false) {
                let transparent = descriptor.fragment.as_ref().unwrap().targets[0]
                    .as_ref()
                    .unwrap()
                    .blend
                    .is_some();
                Specialization.specialize(key, &mut descriptor).unwrap();
                let blend = descriptor.fragment.unwrap().targets[0]
                    .as_ref()
                    .unwrap()
                    .blend
                    .unwrap();
                assert_eq!(blend.color.operation, wgpu::BlendOperation::Add);
                assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::One);
                assert_eq!(
                    blend.color.src_factor,
                    if transparent {
                        wgpu::BlendFactor::SrcAlpha
                    } else {
                        wgpu::BlendFactor::One
                    }
                );
                assert_eq!(blend.alpha, wgpu::BlendComponent::OVER);
            }
        }
    }
}

#[derive(Resource, Default)]
struct Pixel {
    color: [f32; 3],
    depth: f32,
}

/// Models an opaque red grid behind a nearer transparent world surface.
fn opaque_grid(mut pixel: ResMut<Pixel>) {
    if 0.6 >= pixel.depth {
        pixel.color = [1., 0., 0.];
        pixel.depth = 0.6;
    }
}

/// Applies world glass using the terrain pipeline's depth-write contract.
fn world_glass(mut pixel: ResMut<Pixel>) {
    let descriptor = &pipeline::layouts::descriptors::build(false)[3];
    assert_eq!(
        descriptor
            .depth_stencil
            .as_ref()
            .unwrap()
            .depth_write_enabled,
        Some(true)
    );
    if 0.8 >= pixel.depth {
        pixel.color = pixel.color.map(|value| value * 0.5);
        pixel.color[2] += 0.5;
        pixel.depth = 0.8;
    }
}

#[test]
fn retained_opaque_grids_remain_visible_through_world_transparency() {
    use crate::scene_target::ScenePass;
    use bevy::core_pipeline::Core3d;
    let mut world = crate::render_test_support::empty_render_world();
    world.init_resource::<Pixel>();
    install_graph(&mut world);
    world.schedule_scope(Core3d, |_, schedule| {
        schedule
            .configure_sets((ScenePass::Opaque, ScenePass::Transparent, ScenePass::Finish).chain());
        schedule.add_systems((
            opaque_grid.in_set(BlockPass::Opaque),
            world_glass.in_set(ScenePass::Transparent),
        ));
    });
    world.run_schedule(Core3d);
    let pixel = world.resource::<Pixel>();
    assert_eq!(pixel.color, [0.5, 0., 0.5]);
    assert_eq!(pixel.depth, 0.8);
}

#[test]
fn retained_blocks_follow_gameplay_visibility_even_with_active_placements() {
    let (mut app, (view, id), mut placement, _) = placement_fixture();
    placement.active = true;
    app.world_mut().insert_resource(Blocks {
        placements: BTreeMap::from([((view, id), placement)]),
        ..default()
    });
    assert!(drawing::contains(app.world(), view, None, None));
    let mut panorama = render_scene::panorama::PanoramaScene::default();
    panorama.set_game_visible(false);
    app.world_mut().insert_resource(panorama);
    assert!(!drawing::contains(app.world(), view, None, None));
    app.world_mut()
        .resource_mut::<render_scene::panorama::PanoramaScene>()
        .set_game_visible(true);
    assert!(drawing::contains(app.world(), view, None, None));
}

/// Models opaque and blended depth-always contents at the shared overlay boundary.
fn overlay_grid(mut pixel: ResMut<Pixel>) {
    pixel.color = [0.5, 0.5, 0.];
    pixel.depth = 0.6;
}

#[test]
fn retained_depth_always_contents_remain_above_world_glass() {
    use crate::scene_target::ScenePass;
    use bevy::core_pipeline::Core3d;
    let mut world = crate::render_test_support::empty_render_world();
    world.init_resource::<Pixel>();
    install_graph(&mut world);
    world.schedule_scope(Core3d, |_, schedule| {
        schedule
            .configure_sets((ScenePass::Opaque, ScenePass::Transparent, ScenePass::Finish).chain());
        schedule.add_systems((
            world_glass.in_set(ScenePass::Transparent),
            overlay_grid.in_set(BlockPass::Overlay),
        ));
    });
    world.run_schedule(Core3d);
    assert_eq!(world.resource::<Pixel>().color, [0.5, 0.5, 0.]);
}
