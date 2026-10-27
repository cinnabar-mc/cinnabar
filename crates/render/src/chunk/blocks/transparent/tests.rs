use super::*;

/// Supplies two retained section ranges on opposite sides of a world glass surface.
fn fixture() -> (App, bevy::render::view::RetainedViewEntity) {
    let (mut app, (view, id), mut placement) = super::super::tests::placement_fixture();
    let device = app.world().resource::<RenderDevice>();
    let ledger = mod_render::GpuLedger::new(1024);
    placement.order = Some(Order {
        _buffer: ledger
            .allocate(16, || {
                Some(device.create_buffer(&BufferDescriptor {
                    label: None,
                    size: 16,
                    usage: BufferUsages::STORAGE,
                    mapped_at_creation: false,
                }))
            })
            .unwrap(),
        count: 2,
        groups: [10., 25.]
            .into_iter()
            .enumerate()
            .map(|(i, z)| sorting::Group {
                center: Vec3::new(0., 0., z),
                range: i as u32..i as u32 + 1,
            })
            .collect(),
    });
    placement.matrix = Mat4::from_translation(Vec3::new(0., 0., -30.));
    placement.active = true;
    app.add_render_command::<Transparent3d, DrawCommands>();
    let draw_function = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawCommands>();
    let retained = app
        .world()
        .get::<ExtractedView>(view)
        .unwrap()
        .retained_view_entity;
    let main = *app.world().get::<MainEntity>(view).unwrap();
    crate::transparent_phase::add(
        app.world_mut()
            .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
            .get_mut(&retained)
            .unwrap(),
        Transparent3d {
            sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
            distance: -10.,
            pipeline: CachedRenderPipelineId::INVALID,
            entity: (view, main),
            draw_function,
            batch_range: 100..101,
            extra_index: PhaseItemExtraIndex::None,
            indexed: true,
        },
    );
    app.insert_resource(DrawingFrame {
        connection: 1,
        live_owners: [(id.owner, id.generation)].into(),
        ..default()
    })
    .insert_resource(Blocks {
        placements: BTreeMap::from([((view, id), placement)]),
        ..default()
    })
    .add_systems(Update, queue);
    (app, retained)
}

#[test]
fn retained_transparency_composes_on_both_sides_of_depth_writing_world_glass() {
    let (mut app, retained) = fixture();
    app.update();
    let phases = app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>();
    let mut color = [0.; 3];
    let mut depth = 0.;
    for item in phases[&retained].items.values() {
        let (z, source) = match item.distance {
            -20. => (0.2, [1., 0., 0.]),
            -10. => (0.5, [0., 0., 1.]),
            -5. => (0.8, [0., 1., 0.]),
            other => panic!("unexpected transparent section distance: {other}"),
        };
        if z >= depth {
            color = std::array::from_fn(|i| color[i] * 0.5 + source[i] * 0.5);
            depth = z;
        }
    }
    assert_eq!(color, [0.125, 0.5, 0.25]);
    assert_eq!(depth, 0.8);
    assert_eq!(app.world().resource::<Blocks>().draws.len(), 2);
}

#[test]
fn retained_transparency_queues_nothing_under_a_replacement_background() {
    let (mut app, retained) = fixture();
    let mut panorama = render_scene::panorama::PanoramaScene::default();
    panorama.set_game_visible(false);
    app.insert_resource(panorama);
    app.update();
    assert!(app.world().resource::<Blocks>().draws.is_empty());
    assert_eq!(
        app.world()
            .resource::<ViewSortedRenderPhases<Transparent3d>>()[&retained]
            .items
            .len(),
        1
    );
}
