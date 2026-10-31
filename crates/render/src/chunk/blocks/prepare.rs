//! Admits native arenas on workers and refreshes only changed placement inputs.
use super::*;
use bevy::render::render_resource::UniformBuffer;
use gpu::bind_groups::{ChunkGpuAnimationClock, ChunkGpuBiomeTints, ChunkGpuTextureAssets};

/// Serializes one camera in grid space so unchanged native shaders can draw arbitrary transforms.
pub(super) fn uniform(view: &ExtractedView, matrix: Mat4, exposure: f32) -> ViewUniform {
    let world_from_view = matrix.inverse();
    let projection = view.clip_from_view;
    ViewUniform {
        clip_from_world: projection * matrix,
        unjittered_clip_from_world: projection * matrix,
        world_from_clip: world_from_view * projection.inverse(),
        world_from_view,
        view_from_world: matrix,
        clip_from_view: projection,
        view_from_clip: projection.inverse(),
        world_position: world_from_view.w_axis.truncate(),
        exposure,
        viewport: view.viewport.as_vec4(),
        main_pass_viewport: view.viewport.as_vec4(),
        frustum: [bevy::math::Vec4::ZERO; 6],
        lod_view_world_position: world_from_view.w_axis.truncate(),
        color_grading: view.color_grading.clone().into(),
        mip_bias: 0.,
        frame_count: 0,
    }
}

/// Resolves current grid coordinates into view space for both sorting and GPU uniforms.
pub(super) fn matrix(
    view: &ExtractedView,
    resolved: &render_scene::mod_scene::retained::GpuInstance,
) -> Option<Mat4> {
    let rotation = view.world_from_view.to_matrix();
    let rotation = Mat4::from_cols(
        rotation.x_axis,
        rotation.y_axis,
        rotation.z_axis,
        bevy::math::Vec4::W,
    )
    .inverse();
    let matrix = rotation * Mat4::from_cols_array_2d(&resolved.transform);
    let determinant = matrix.determinant();
    (determinant.is_finite() && determinant.abs() >= 1e-10).then_some(matrix)
}

/// Limits uploads to one background generation, then binds cached shared terrain tables.
#[allow(
    clippy::too_many_arguments,
    reason = "native terrain preparation borrows its canonical render resources"
)]
pub(super) fn prepare(
    scene: Res<ModRenderScene>,
    frame: Res<DrawingFrame>,
    textures: Option<Res<ChunkGpuTextureAssets>>,
    assets: Option<Res<ChunkTextureAssets>>,
    clock: Option<Res<ChunkGpuAnimationClock>>,
    tints: Option<Res<ChunkGpuBiomeTints>>,
    atmosphere: Option<Res<AtmosphereGpu>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<Pipelines>,
    mut blocks: ResMut<Blocks>,
    views: Query<(Entity, &ExtractedView, &ExtractedCamera, &Msaa)>,
) {
    for placement in blocks.placements.values_mut() {
        placement.active = false;
    }
    if frame.connection == 0 {
        let native = std::mem::take(&mut blocks.native);
        let placements = std::mem::take(&mut blocks.placements);
        let done: Vec<_> = blocks
            .pending
            .iter_mut()
            .filter_map(|(id, pending)| {
                block_on(poll_once(&mut pending.task)).map(|result| (*id, result))
            })
            .collect();
        let mut uploads = Vec::new();
        for (id, result) in done {
            blocks.pending.remove(&id);
            if let Some(result) = result {
                uploads.push(result);
            }
        }
        blocks.ledgers.clear();
        if !native.is_empty() || !placements.is_empty() || !uploads.is_empty() {
            crate::device_poll::on_frame_complete(&queue, move || {
                drop((native, placements, uploads))
            });
        }
        return;
    }
    let (Some(textures), Some(assets), Some(clock), Some(tints), Some(atmosphere)) =
        (textures, assets, clock, tints, atmosphere)
    else {
        return;
    };
    let (Some(textures), Some(tints)) = (textures.prepared.as_ref(), tints.prepared.as_ref())
    else {
        return;
    };
    admit_triangles(&scene);
    let live: BTreeMap<_, _> = scene
        .retained
        .iter()
        .filter(|r| r.connection == frame.connection && r.dimension == frame.dimension)
        .flat_map(|r| r.geometry.iter())
        .filter(|(id, g)| {
            g.blocks().is_some() && frame.live_owners.contains(&(id.owner, id.generation))
        })
        .map(|(id, g)| (*id, Arc::clone(g)))
        .collect();
    let removed: Vec<_> = blocks
        .native
        .keys()
        .filter(|id| {
            live.get(id)
                .is_none_or(|g| !Arc::ptr_eq(g, &blocks.native[id].geometry))
        })
        .copied()
        .collect();
    let mut retired = Vec::new();
    for id in removed {
        if let Some(old) = blocks.native.remove(&id) {
            retired.push(old);
        }
    }
    let done: Vec<_> = blocks
        .pending
        .iter_mut()
        .filter_map(|(id, pending)| {
            block_on(poll_once(&mut pending.task)).map(|native| (*id, native))
        })
        .collect();
    for (id, native) in done {
        let pending = blocks.pending.remove(&id).unwrap();
        if let Some(native) = native {
            if live
                .get(&id)
                .is_some_and(|g| Arc::ptr_eq(g, &pending.geometry))
            {
                if let Some(request) = native.geometry.blocks() {
                    request.complete_render();
                }
                blocks.native.insert(id, Arc::new(native));
            } else {
                retired.push(Arc::new(native));
            }
        } else {
            fail(&pending.geometry, "block GPU arena");
        }
    }
    if blocks.pending.is_empty() {
        for (&id, geometry) in &live {
            if blocks.native.contains_key(&id)
                || geometry.blocks().unwrap().render_error().is_some()
            {
                continue;
            }
            let Some(Ok(mesh)) = geometry.blocks().unwrap().result() else {
                continue;
            };
            if device.limits().max_storage_buffers_per_shader_stage
                < required_vertex_storage_buffers()
            {
                fail(geometry, "block GPU storage support");
                continue;
            }
            let mesh = Arc::clone(mesh);
            let ledger = mod_render::GpuReservation::ledger_for(&scene.gpu_reservations, id.owner)
                .cloned()
                .unwrap_or_else(|| {
                    blocks
                        .ledgers
                        .entry(id.owner)
                        .or_insert_with(|| {
                            mod_render::GpuLedger::new(mod_api::drawing::MAX_BLOCK_GPU_BYTES)
                        })
                        .clone()
                });
            let Some(charge) = ledger.allocate(
                (mesh.bytes as u64)
                    .saturating_mul(4)
                    .saturating_add(2 * 1024 * 1024),
                || Some(()),
            ) else {
                fail(geometry, "block GPU memory");
                continue;
            };
            let (geometry, assets, device, queue) = (
                Arc::clone(geometry),
                assets.clone(),
                device.clone(),
                queue.clone(),
            );
            let retained = Arc::clone(&geometry);
            let task = AsyncComputeTaskPool::get().spawn(async move {
                let instances = instances(&mesh);
                let terrain = PreparedResourceGeometry::build(
                    &instances,
                    assets.clone(),
                    device,
                    queue,
                    None,
                )?;
                let faces = sorting::faces(&instances, &terrain, assets.assets())?;
                Some(Native {
                    geometry,
                    terrain,
                    faces,
                    _charge: charge,
                })
            });
            blocks.pending.insert(
                id,
                Pending {
                    geometry: retained,
                    task,
                },
            );
            break;
        }
    }
    let mut placements = std::mem::take(&mut blocks.placements);
    for placement in placements.values_mut() {
        placement.active = false;
    }
    let removed: Vec<_> = placements
        .iter()
        .filter(|((view, id), placement)| {
            views.get(*view).is_err()
                || !placement_is_current(&scene.retained, &live, *id, &placement.native.geometry)
        })
        .map(|(key, _)| *key)
        .collect();
    let mut retired_placements = Vec::new();
    let mut retired_orders = Vec::new();
    for key in removed {
        if let Some(old) = placements.remove(&key) {
            retired_placements.push(old);
        }
    }
    poll_orders(&mut placements, &mut retired_orders);
    let mut sort_requests = Vec::new();
    for resources in &scene.retained {
        if resources.connection != frame.connection || resources.dimension != frame.dimension {
            continue;
        }
        for (&id, instance) in &resources.instances {
            let Some(native) = blocks.native.get(&instance.geometry).cloned() else {
                continue;
            };
            if native
                .geometry
                .blocks()
                .is_some_and(|block| block.render_error().is_some())
            {
                continue;
            }
            let ledger = mod_render::GpuReservation::ledger_for(&scene.gpu_reservations, id.owner)
                .cloned()
                .unwrap_or_else(|| {
                    blocks
                        .ledgers
                        .entry(id.owner)
                        .or_insert_with(|| {
                            mod_render::GpuLedger::new(mod_api::drawing::MAX_BLOCK_GPU_BYTES)
                        })
                        .clone()
                });
            for (entity, view, camera, msaa) in &views {
                let Some(resolved) = resolve(instance, &frame, &view.world_from_view) else {
                    continue;
                };
                if !render_scene::mod_scene::retained::visible(
                    &native.geometry,
                    &resolved,
                    &view.world_from_view,
                    view.clip_from_view,
                ) {
                    continue;
                }
                let Some(matrix) = matrix(view, &resolved) else {
                    continue;
                };
                let determinant = matrix.determinant();
                let key = Key {
                    msaa: *msaa,
                    hdr: camera.hdr,
                    clipped: instance.clip.is_some(),
                    always: instance.depth == Depth::Always,
                    additive: instance.blend == Blend::Additive,
                    mirrored: determinant < 0.,
                };
                let ids: Option<Vec<_>> = pipelines
                    .variants
                    .iter_mut()
                    .map(|variants| variants.specialize(&cache, key).ok())
                    .collect();
                let Some(ids) = ids else {
                    continue;
                };
                let key = (entity, id);
                if placements
                    .get(&key)
                    .is_some_and(|p| !Arc::ptr_eq(&p.native, &native))
                    && let Some(old) = placements.remove(&key)
                {
                    retired_placements.push(old);
                }
                if let std::collections::btree_map::Entry::Vacant(entry) = placements.entry(key) {
                    let Some(charge) = ledger.allocate(ViewUniform::min_size().get(), || Some(()))
                    else {
                        fail(&native.geometry, "block view memory");
                        continue;
                    };
                    let mut uniform = UniformBuffer::from(uniform(view, matrix, camera.exposure));
                    uniform.write_buffer(&device, &queue);
                    entry.insert(Placement {
                        uniform,
                        matrix,
                        projection: view.clip_from_view,
                        viewport: view.viewport,
                        exposure: camera.exposure,
                        binding: None,
                        opaque_binding: None,
                        identity: None,
                        native: Arc::clone(&native),
                        order: None,
                        order_job: None,
                        sort_camera: None,
                        pipelines: ids.clone().try_into().unwrap(),
                        clip: instance.clip,
                        always: instance.depth == Depth::Always,
                        active: true,
                        _charge: charge,
                    });
                }
                let placement = placements.get_mut(&key).unwrap();
                placement.pipelines = ids.try_into().unwrap();
                placement.clip = instance.clip;
                placement.always = instance.depth == Depth::Always;
                placement.active = true;
                if placement.matrix != matrix
                    || placement.projection != view.clip_from_view
                    || placement.viewport != view.viewport
                    || placement.exposure != camera.exposure
                {
                    placement
                        .uniform
                        .set(uniform(view, matrix, camera.exposure));
                    placement.uniform.write_buffer(&device, &queue);
                    placement.matrix = matrix;
                    placement.projection = view.clip_from_view;
                    placement.viewport = view.viewport;
                    placement.exposure = camera.exposure;
                }
                let identity = (textures.identity, tints.buffer.id(), atmosphere.buffer.id());
                if placement.identity != Some(identity) {
                    let mut entries = gpu::bind_groups::entries(
                        placement.uniform.binding().unwrap(),
                        &native.terrain.arena,
                        textures,
                        &clock,
                        tints,
                        &atmosphere,
                    );
                    if let Some(order) = &placement.order
                        && let Some(entry) = entries
                            .iter_mut()
                            .find(|entry| entry.binding == TRANSPARENT_REFS_BINDING)
                    {
                        entry.resource = order._buffer.as_entire_binding();
                    }
                    placement.binding = Some(device.create_bind_group(
                        "block native terrain",
                        &cache.get_bind_group_layout(&pipeline::layouts::chunk_bind_group_layout()),
                        &entries,
                    ));
                    let entries: Vec<_> = entries
                        .into_iter()
                        .filter(|e| e.binding != TRANSPARENT_REFS_BINDING)
                        .collect();
                    placement.opaque_binding = Some(device.create_bind_group(
                        "block native opaque terrain",
                        &cache.get_bind_group_layout(
                            &pipeline::layouts::opaque_chunk_bind_group_layout(),
                        ),
                        &entries,
                    ));
                    placement.identity = Some(identity);
                }
                let local_camera = matrix.inverse().w_axis.truncate();
                let direction = -matrix.row(2).truncate().normalize_or_zero();
                let camera_key =
                    crate::chunk::transparent::face_metric::quantized_position_bits(local_camera)
                        .map(|p| {
                            [
                                p[0],
                                p[1],
                                p[2],
                                direction.x.to_bits(),
                                direction.y.to_bits(),
                                direction.z.to_bits(),
                            ]
                        });
                if placement.order_job.is_none()
                    && placement.sort_camera != camera_key
                    && !native.faces.is_empty()
                {
                    sort_requests.push((key, camera_key, local_camera, direction, ledger.clone()));
                }
            }
        }
    }
    if !placements
        .values()
        .any(|placement| placement.order_job.is_some())
        && !sort_requests.is_empty()
    {
        sort_requests.sort_by_key(|request| request.0);
        let index = sort_requests
            .iter()
            .position(|request| blocks.sort_cursor.is_none_or(|cursor| request.0 > cursor))
            .unwrap_or(0);
        let (key, camera_key, local_camera, direction, ledger) = sort_requests.swap_remove(index);
        let placement = placements.get_mut(&key).unwrap();
        let (native, device, queue) =
            (Arc::clone(&placement.native), device.clone(), queue.clone());
        placement.sort_camera = camera_key;
        placement.order_job = Some(AsyncComputeTaskPool::get().spawn(async move {
            sorting::sort(native, local_camera, direction, device, queue, &ledger)
        }));
        blocks.sort_cursor = Some(key);
    }
    blocks
        .ledgers
        .retain(|owner, _| live.keys().any(|id| id.owner == *owner));
    blocks.placements = placements;
    if !retired.is_empty() || !retired_placements.is_empty() || !retired_orders.is_empty() {
        crate::device_poll::on_frame_complete(&queue, move || {
            drop((retired, retired_placements, retired_orders))
        });
    }
}

/// Keeps a placement only while its instance still names the same live grid generation.
pub(super) fn placement_is_current(
    resources: &[Arc<mod_render::retained::Resources>],
    live: &BTreeMap<DrawingHandle, Arc<mod_render::retained::Geometry>>,
    id: DrawingHandle,
    geometry: &Arc<mod_render::retained::Geometry>,
) -> bool {
    resources
        .iter()
        .filter_map(|resources| resources.instances.get(&id))
        .any(|instance| {
            live.get(&instance.geometry)
                .is_some_and(|current| Arc::ptr_eq(current, geometry))
        })
}

/// Validates completed triangle work before native resources become drawable.
pub(super) fn admit_triangles(scene: &ModRenderScene) {
    for resources in &scene.retained {
        let triangles = resources
            .instances
            .values()
            .filter_map(|instance| {
                resources
                    .geometry
                    .get(&instance.geometry)?
                    .blocks()?
                    .result()?
                    .as_ref()
                    .ok()
            })
            .fold(0usize, |sum, mesh| sum.saturating_add(mesh.triangles));
        if triangles > mod_api::drawing::MAX_BLOCK_DRAW_TRIANGLES {
            for geometry in resources
                .geometry
                .values()
                .filter(|geometry| geometry.blocks().is_some())
            {
                fail(geometry, "block draw triangles");
            }
        }
    }
    for reservation in &scene.gpu_reservations {
        if reservation.retained_triangles_exceeded(&scene.retained) {
            for resources in &scene.retained {
                for (id, geometry) in &resources.geometry {
                    if reservation.owners.contains(&id.owner) && geometry.blocks().is_some() {
                        fail(geometry, "bundle triangle work");
                    }
                }
            }
        }
    }
}

/// Releases completed sorting jobs independently of future drawing demand.
pub(super) fn poll_orders(
    placements: &mut BTreeMap<(Entity, DrawingHandle), Placement>,
    retired: &mut Vec<Order>,
) {
    for placement in placements.values_mut() {
        if let Some(job) = placement.order_job.as_mut()
            && let Some(order) = block_on(poll_once(job))
        {
            placement.order_job = None;
            if let Some(order) = order {
                if let Some(old) = placement.order.replace(order) {
                    retired.push(old);
                }
                placement.identity = None;
            } else {
                fail(&placement.native.geometry, "block transparent order memory");
            }
        }
    }
}
