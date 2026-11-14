use super::*;
use crate::chunk::gpu_cull::app_tests::{
    KEYS, camera_transform, chunk_app, frame, insert_meshes, noop_render_plugin,
};

#[derive(Resource, Default)]
struct LegacyDraws {
    quads: [Vec<(u32, u32)>; 4],
    items: usize,
    draws: usize,
}

/// Expands the actual per-section phase order into origin/quad addresses for comparison.
fn capture_legacy(
    phases: Res<ViewBinnedRenderPhases<Opaque3d>>,
    functions: Res<DrawFunctions<Opaque3d>>,
    allocations: Query<&GpuChunkAllocation>,
    views: Query<&ExtractedView>,
    mut capture: ResMut<LegacyDraws>,
) {
    *capture = default();
    let functions = functions.read();
    let view = views.iter().next().unwrap();
    let camera = pipeline::solid::solid_cull_camera(view, false);
    for phase in phases.0.values() {
        for ((key, _), bin) in &phase.non_mesh_items {
            let stream = match pipeline::opaque::timing_category(&functions, key.draw_function) {
                RuntimeStage::GpuTerrainSolid => 0,
                RuntimeStage::GpuTerrainCutout => 1,
                RuntimeStage::GpuTerrainModel => 2,
                RuntimeStage::GpuTerrainDepthLiquid => 3,
                _ => continue,
            };
            for entity in bin.entities.values() {
                let Ok(allocation) = allocations.get(*entity) else {
                    continue;
                };
                capture.items += 1;
                let commands = match stream {
                    0 => solid_indirect_commands(allocation, camera)
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>(),
                    1 => cutout_indirect_command(allocation).into_iter().collect(),
                    2 => model_direct_draw_command(allocation).into_iter().collect(),
                    _ => depth_liquid_direct_draw_command(allocation)
                        .into_iter()
                        .collect(),
                };
                capture.draws += commands.len();
                for command in commands {
                    capture.quads[stream].extend(
                        (command.first_instance..command.first_instance + command.instance_count)
                            .map(move |quad| (allocation.metadata_index, quad)),
                    );
                }
            }
        }
    }
}

#[test]
fn direct_batches_collapse_section_draws_and_keep_each_sections_quad_order() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Off,
        camera_transform(),
    );
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<gpu_cull::DirectOcclusionSupport>()
        .0 = false;
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<TerrainUnitBatches>()
        .disabled = true;
    app.sub_app_mut(RenderApp)
        .init_resource::<LegacyDraws>()
        .add_systems(
            Render,
            capture_legacy
                .in_set(RenderSystems::Queue)
                .after(crate::chunk::draw::queue_chunks),
        );
    insert_meshes(&mut app, &KEYS);
    for _ in 0..8 {
        frame(&mut app);
    }
    let legacy = app.sub_app(RenderApp).world().resource::<LegacyDraws>();
    let (expected, items, draws) = (legacy.quads.clone(), legacy.items, legacy.draws);
    assert!(draws >= KEYS.len());
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<TerrainUnitBatches>()
        .disabled = false;
    frame(&mut app);
    let batches = app
        .sub_app(RenderApp)
        .world()
        .resource::<TerrainUnitBatches>();
    assert!(
        batches.stats.phase_items < items as u64,
        "{:?}",
        batches.stats
    );
    assert!(batches.stats.draws < draws as u64, "{:?}", batches.stats);
    assert!(batches.stats.list_bytes_uploaded > 0);
    assert_eq!(batches.views.len(), 1);
    let view = batches.views.values().next().unwrap();
    // Batches may order sections differently; each section's quads keep their exact order.
    let by_section = |mut quads: Vec<(u32, u32)>| {
        quads.sort_by_key(|&(origin, _)| origin);
        quads
    };
    for (stream, expected) in view.streams.iter().zip(&expected) {
        let actual = stream
            .visible_units()
            .flat_map(|unit| {
                (0..unit.quad_count).map(move |quad| (unit.origin_slot, unit.stream_start + quad))
            })
            .collect::<Vec<_>>();
        assert_eq!(by_section(actual), by_section(expected.clone()));
    }
    frame(&mut app);
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .resource::<TerrainUnitBatches>()
            .stats
            .list_bytes_uploaded,
        0
    );
    insert_meshes(&mut app, &KEYS[..1]);
    for _ in 0..4 {
        frame(&mut app);
    }
    let world = app.sub_app_mut(RenderApp).world_mut();
    let current = world
        .query::<&GpuChunkAllocation>()
        .iter(world)
        .map(|allocation| (allocation.key, allocation.generation))
        .collect::<HashMap<_, _>>();
    let batches = world.resource::<TerrainUnitBatches>();
    for view in batches.views.values() {
        for section in view.sources.streams.iter().flatten() {
            assert!(
                section
                    .units
                    .iter()
                    .all(|unit| unit.allocation_generation == current[&section.key])
            );
        }
    }
}

#[test]
fn unit_indices_keep_the_quad_winding_and_fit_the_existing_storage_budget() {
    assert_eq!(indices().len(), index_count() as usize);
    let all = indices();
    let (quads, rest) = all.as_chunks::<{ STATIC_QUAD_INDICES.len() }>();
    assert!(rest.is_empty());
    for (quad, indices) in quads.iter().enumerate() {
        assert_eq!(
            *indices,
            STATIC_QUAD_INDICES.map(|index| quad as u32 * 4 + index)
        );
    }
    let storage_slots = pipeline::layouts::opaque_chunk_bind_group_layout()
        .entries
        .iter()
        .chain(bind_group_layout().entries.iter())
        .filter(|entry| {
            entry.visibility.contains(ShaderStages::VERTEX)
                && matches!(
                    entry.ty,
                    BindingType::Buffer {
                        ty: BufferBindingType::Storage { .. },
                        ..
                    }
                )
        })
        .count() as u32;
    assert_eq!(storage_slots, required_vertex_storage_buffers());
    let mut limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: storage_slots,
        ..default()
    };
    assert!(supported(ChunkDrawMode::Direct, &limits));
    assert!(!supported(ChunkDrawMode::MultiDrawIndirect, &limits));
    limits.max_storage_buffers_per_shader_stage -= 1;
    assert!(!supported(ChunkDrawMode::Direct, &limits));
}

#[derive(Resource, Default)]
struct PassOrderChecks {
    early: usize,
    ordinary: usize,
}

/// Checks selected solid quads against the current early-pass order before that plan is consumed.
fn check_solid_pass_order(
    batches: Res<TerrainUnitBatches>,
    occlusion: Res<gpu_cull::direct::DirectOcclusion>,
    allocations: Query<&GpuChunkAllocation>,
    mut checks: ResMut<PassOrderChecks>,
) {
    for (&entity, view) in &batches.views {
        let batch = &view.streams[0];
        let Some(order) = occlusion.solid_order(entity) else {
            checks.ordinary += 1;
            continue;
        };
        checks.early += 1;
        let expected = order
            .iter()
            .flat_map(|&(entity, _)| {
                let allocation = allocations.get(entity).unwrap();
                solid_indirect_commands(allocation, view.camera)
                    .into_iter()
                    .flatten()
                    .flat_map(|command| {
                        (command.first_instance..command.first_instance + command.instance_count)
                            .map(move |quad| (allocation.metadata_index, quad))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let actual = batch
            .visible_units()
            .flat_map(|unit| {
                (0..unit.quad_count).map(move |quad| (unit.origin_slot, unit.stream_start + quad))
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}

#[test]
fn direct_occlusion_passes_keep_their_order_when_batched() {
    let (mut app, camera) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Off,
        camera_transform(),
    );
    app.sub_app_mut(RenderApp)
        .init_resource::<PassOrderChecks>()
        .add_systems(
            Render,
            check_solid_pass_order
                .in_set(RenderSystems::PrepareResources)
                .after(super::prepare::prepare_units),
        );
    insert_meshes(&mut app, &KEYS);
    for _ in 0..8 {
        frame(&mut app);
    }
    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .x += 8.0;
    for _ in 0..8 {
        frame(&mut app);
    }
    let checks = app.sub_app(RenderApp).world().resource::<PassOrderChecks>();
    assert!(checks.early > 0);
    assert!(checks.ordinary > 0);
}

/// Provides valid, disjoint cube, model and liquid streams with tails across multiple units.
fn mixed_allocation() -> GpuChunkAllocation {
    GpuChunkAllocation {
        key: SubChunkKey::new(0, 0, 0, 0),
        generation: 17,
        tint_identity: ChunkBiomeTintIdentity::default(),
        quad_range: 100..290,
        cube_layout: CubeQuadLayout::from_solid_counts([10, 30, 0, 1, 65, 0]),
        cube_lighting_range: Some(600..980),
        model_range: Some(1000..1040),
        model_lighting_range: Some(1040..1070),
        model_draw_range: Some(1070..1330),
        transparent_model_draw_range: None,
        liquid_range: Some(2000..2532),
        liquid_lighting_range: Some(2532..2798),
        has_depth_liquid: true,
        has_transparent_liquid: true,
        depth_liquid_range: Some(501..633),
        order_independent_liquid: false,
        metadata_index: 7,
    }
}

#[test]
fn descriptors_preserve_every_validated_stream_and_facing_class() {
    let allocation = mixed_allocation();
    for camera in [None, Some([-2.0; 3]), Some([20.0; 3]), Some([8.0; 3])] {
        let facing = camera.map_or(meshing::FaceMask::ALL, |eye| {
            meshing::sub_chunk_facing_faces([0; 3], eye)
        });
        let expected = [
            solid_indirect_commands(&allocation, camera)
                .unwrap()
                .collect::<Vec<_>>(),
            cutout_indirect_command(&allocation).into_iter().collect(),
            model_direct_draw_command(&allocation).into_iter().collect(),
            depth_liquid_direct_draw_command(&allocation)
                .into_iter()
                .collect(),
        ];
        for (stream, commands) in expected.into_iter().enumerate() {
            let descriptors = sources::allocation_units(&allocation, stream);
            assert!(
                descriptors
                    .iter()
                    .all(|unit| unit.origin_slot == allocation.metadata_index
                        && unit.allocation_generation == allocation.generation
                        && unit.quad_count <= QUADS_PER_UNIT)
            );
            let mut visible = Vec::new();
            render_model::terrain_units::TerrainUnit::compact(
                &descriptors,
                |unit| {
                    unit.facing
                        .is_none_or(|face| facing.contains(meshing::Face::ALL[face as usize]))
                },
                &mut visible,
            );
            let actual = visible
                .iter()
                .flat_map(|unit| unit.stream_start..unit.stream_start + unit.quad_count)
                .collect::<Vec<_>>();
            let expected = commands
                .iter()
                .flat_map(|command| {
                    command.first_instance..command.first_instance + command.instance_count
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "stream {stream}, camera {camera:?}");
        }
    }
}

/// Builds source publications through the production allocation query and validation.
fn source_snapshot(
    world: &mut World,
    entities: &[Entity],
    previous: Option<&UnitSources>,
) -> UnitSources {
    let mut state = bevy::ecs::system::SystemState::<Query<&GpuChunkAllocation>>::new(world);
    let allocations = state.get(world).unwrap();
    let drawable = entities
        .iter()
        .map(|&entity| {
            crate::chunk::pipeline::batches::DrawableTerrain::new(
                entity,
                MainEntity::from(entity),
                allocations.get(entity).unwrap(),
                true,
            )
        })
        .collect::<Vec<_>>();
    UnitSources::build(&drawable, &allocations, previous)
}

#[test]
fn source_refresh_reuses_unchanged_descriptors_and_invalidates_reused_addresses() {
    let mut world = World::new();
    let [a, b] = [(); 2].map(|_| world.spawn(mixed_allocation()).id());
    let first = source_snapshot(&mut world, &[a, b], None);
    world.get_mut::<GpuChunkAllocation>(a).unwrap().generation += 1;
    let changed = source_snapshot(&mut world, &[a, b], Some(&first));
    for stream in 0..4 {
        let old_a = &first.streams[stream][first.lookup[stream][&a]];
        let new_a = &changed.streams[stream][changed.lookup[stream][&a]];
        let old_b = &first.streams[stream][first.lookup[stream][&b]];
        let new_b = &changed.streams[stream][changed.lookup[stream][&b]];
        assert!(!Arc::ptr_eq(old_a, new_a));
        assert!(Arc::ptr_eq(old_b, new_b));
        assert!(
            new_a
                .units
                .iter()
                .all(|unit| unit.allocation_generation == 18)
        );
    }
    // Metadata relocation must refresh even if the mesh generation is unchanged.
    world
        .get_mut::<GpuChunkAllocation>(a)
        .unwrap()
        .metadata_index = 21;
    let relocated = source_snapshot(&mut world, &[a, b], Some(&changed));
    for stream in 0..4 {
        let source = &relocated.streams[stream][relocated.lookup[stream][&a]];
        assert!(source.units.iter().all(|unit| unit.origin_slot == 21));
    }
    // A removed source cannot remain reachable through the cached section lookup.
    let removed = source_snapshot(&mut world, &[b], Some(&relocated));
    assert!(removed.lookup.iter().all(|lookup| !lookup.contains_key(&a)));
}

impl TerrainUnitBatches {
    /// Reads what each direct-draw page submits: its reference count and the descriptors its
    /// references resolve to, in draw order. Descriptor slots are stable across frames, so
    /// their order and any unreferenced slot or tail depend on history rather than on the
    /// publication the shader draws.
    pub(in crate::chunk) fn publication_snapshot(
        &self,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Vec<Vec<u8>> {
        let mut views = self.views.iter().collect::<Vec<_>>();
        views.sort_unstable_by_key(|(entity, _)| **entity);
        views
            .into_iter()
            .flat_map(|(_, view)| view.streams.iter().chain(std::iter::once(&view.occluders)))
            .flat_map(|stream| &stream.pages)
            .map(|page| {
                let bytes = crate::render_test_support::read_buffer(
                    device,
                    queue,
                    &page.buffer,
                    0,
                    page.buffer.size(),
                );
                let records: &[VisibleTerrainUnit] = bytemuck::cast_slice(&bytes);
                let [references, count, ..] = bytemuck::cast::<_, [u32; 4]>(records[0]);
                let references: &[u32] = bytemuck::cast_slice(&records[references as usize..]);
                let mut submitted = count.to_le_bytes().to_vec();
                for &slot in &references[..count as usize] {
                    submitted.extend_from_slice(bytemuck::bytes_of(&records[1 + slot as usize]));
                }
                submitted
            })
            .collect()
    }
}

#[test]
fn opaque_streams_draw_solid_before_cutout_whichever_section_comes_first() {
    let mut world = World::new();
    let [cutout_only, mixed] = [(); 2].map(|_| world.spawn(mixed_allocation()).id());
    let mut state = bevy::ecs::system::SystemState::<Query<&GpuChunkAllocation>>::new(&mut world);
    let allocations = state.get(&world).unwrap();
    let drawable = [cutout_only, mixed].map(|entity| {
        crate::chunk::pipeline::batches::DrawableTerrain::new(
            entity,
            MainEntity::from(entity),
            allocations.get(entity).unwrap(),
            true,
        )
    });
    let mut drawable = drawable.to_vec();
    drawable[0].solid = false;
    drawable[0].model = false;
    drawable[0].depth_liquid = false;
    assert!(drawable[0].cutout && drawable[1].solid);
    let sources = UnitSources::build(&drawable, &allocations, None);
    assert_eq!(sources.order, [0, 1, 2, 3]);
}

#[test]
fn sections_are_ordered_nearest_first_from_the_eye() {
    let mut world = World::new();
    let sections = [3, 0, 5, 1, 4, 2].map(|x| {
        super::sources::SectionUnits::fixture_at(
            world.spawn_empty().id(),
            SubChunkKey::new(0, x, 0, 0),
            Vec::new(),
        )
    });
    let order = super::sources::near_first(&sections, [-4.0, 8.0, 8.0]);
    let xs = order
        .iter()
        .map(|entity| {
            let section = sections.iter().find(|s| s.entity == *entity).unwrap();
            section.key.x
        })
        .collect::<Vec<_>>();
    assert_eq!(xs, [0, 1, 2, 3, 4, 5]);
}

#[test]
fn prepared_solid_sections_are_submitted_nearest_first() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Off,
        camera_transform(),
    );
    // Without the occlusion terrain pass, the solid stream uses the ordinary section order.
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<gpu_cull::DirectOcclusionSupport>()
        .0 = false;
    let keys = (0..3)
        .flat_map(|x| (0..3).map(move |z| SubChunkKey::new(0, x, 0, z)))
        .collect::<Vec<_>>();
    insert_meshes(&mut app, &keys);
    for _ in 0..8 {
        frame(&mut app);
    }
    let batches = app
        .sub_app(RenderApp)
        .world()
        .resource::<TerrainUnitBatches>();
    let view = batches.views.values().next().unwrap();
    let eye = view.camera.unwrap();
    let distance = |entity: &Entity| {
        let section = &view.sources.streams[0][view.sources.lookup[0][entity]];
        let origin = chunk_origin(section.key);
        (0..3)
            .map(|axis| f64::from(origin[axis]) + 8.0 - eye[axis])
            .map(|delta| delta * delta)
            .sum::<f64>()
    };
    let selected = &view.streams[0].selected;
    assert!(selected.len() >= 3, "{}", selected.len());
    assert!(
        selected
            .windows(2)
            .all(|pair| distance(&pair[0]) <= distance(&pair[1])),
        "{:?}",
        selected.iter().map(distance).collect::<Vec<_>>()
    );
    // The source lists keep hash order, so this ordering comes from preparation.
    let source = view.sources.streams[0]
        .iter()
        .map(|section| section.entity)
        .collect::<Vec<_>>();
    assert!(
        !source
            .windows(2)
            .all(|pair| distance(&pair[0]) <= distance(&pair[1]))
    );
}
