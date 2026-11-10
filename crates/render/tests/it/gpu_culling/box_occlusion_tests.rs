//! Current depth owns every raster verdict, even with stale or reused slot history.
use super::*;

/// A small projected box, with distinct ranges in every opaque stream.
fn record(bounds: [[i32; 3]; 2]) -> CullRecord {
    CullRecord::new(&CullRecordSource {
        bounds,
        cube: 0..2,
        solid_ends: [1; 6],
        model: 2..3,
        liquid: 3..4,
        ..Default::default()
    })
    .unwrap()
}

/// Projects integer box bounds into a small central rectangle and ordinary reversed depth.
fn input(slots: u32) -> CullViewUniform {
    let mut input = view_input(&camera(Vec3::ZERO, Vec3::NEG_Z), 0);
    input.clip_from_world = Mat4::from_scale(Vec3::new(0.1, 0.1, 0.1))
        .as_dmat4()
        .to_cols_array_2d();
    let mut uniform = CullViewUniform::new(
        &input,
        CullPhase::Late,
        slots,
        slots.next_power_of_two(),
        model::CULL_WORKGROUP,
    );
    uniform.planes = [[0.0; 4]; 5];
    uniform.depth[3] = model::OCCLUSION_RASTER;
    uniform
}

/// Clears all samples, without any geometry or colour draw that could obscure the verdict.
fn clear_depth(gpu: &Gpu, target: &Target, depth: f32) {
    let view = target.depth.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("raster occlusion depth control"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(depth),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    }));
    gpu.queue.submit([encoder.finish()]);
}

#[test]
fn raster_boxes_reset_stale_visibility_and_preserve_depth_at_every_sample_count() {
    let Some((gpu, adapter)) = Gpu::for_fixture_with_adapter(
        "raster box occlusion",
        wgpu::Features::SHADER_EARLY_DEPTH_TEST
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        wgpu::Limits::default(),
    ) else {
        return;
    };
    if gpu.backend != wgpu::Backend::Vulkan
        || !gpu
            .device
            .features()
            .contains(wgpu::Features::SHADER_EARLY_DEPTH_TEST)
    {
        eprintln!("missing fixture: Vulkan forced early-depth raster occlusion");
        return;
    }
    let records = [
        record([[0, 0, 2], [1, 1, 3]]),
        record([[0, 0, 9], [1, 1, 10]]),
        record([[0, 0, 2], [20, 20, 3]]),
    ];
    let culler = Culler::new(&gpu, &records, &[7]);
    let uniform = input(records.len() as u32);
    culler
        .storage
        .write_uniform(&gpu.queue, CullPhase::Late, &uniform);
    for samples in [1, 2, 4, 8] {
        if !adapter
            .get_texture_format_features(DEPTH_FORMAT)
            .flags
            .sample_count_supported(samples)
        {
            eprintln!("missing fixture: {samples}x depth MSAA");
            continue;
        }
        let target = Target::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, samples);
        let raster = box_occlusion::RasterCull::new(
            &gpu.device,
            &culler.storage,
            culler.kernels.workgroup,
            DEPTH_FORMAT,
            samples,
        );
        for depth in [0.8, 0.0, 0.8] {
            clear_depth(&gpu, &target, depth);
            culler.set_history(&[model::HISTORY_RASTER_VISIBLE; 3]);
            let before = sampled_depth(&gpu, &target);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            raster.encode(
                &mut encoder,
                &target.depth.create_view(&Default::default()),
                records.len() as u32,
                uniform.viewport,
                (None, None),
            );
            gpu.queue.submit([encoder.finish()]);
            let bytes = read_buffer(&gpu, &culler.storage.history, 12);
            let words: &[u32] = bytemuck::cast_slice(&bytes);
            assert_eq!(
                words,
                &[
                    if depth == 0.0 {
                        model::HISTORY_VISIBLE | model::HISTORY_RASTER_VISIBLE
                    } else {
                        model::HISTORY_VISIBLE
                    },
                    model::HISTORY_VISIBLE | model::HISTORY_RASTER_VISIBLE,
                    model::HISTORY_VISIBLE | model::HISTORY_RASTER_VISIBLE
                ],
                "{samples} samples at {depth}"
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let groups = culler
                .kernels
                .bind_groups(&gpu.device, &culler.storage, None);
            culler
                .kernels
                .encode_cull(&mut encoder, &groups[1], culler.slots, None);
            gpu.queue.submit([encoder.finish()]);
            let expected = reference_args(
                &records,
                &culler.order,
                CullCamera::new(Some([0.0; 3])),
                INDEX_COUNTS,
                |slot| slot != 0 || depth == 0.0,
            );
            assert_eq!(
                culler.args(CullPhase::Late),
                expected,
                "first-frame late recovery with {samples} samples at depth {depth}"
            );
            let after = sampled_depth(&gpu, &target);
            let different = after.iter().zip(&before).filter(|(a, b)| a != b).count();
            assert_eq!(different, 0, "box tests must not write any depth sample");
        }
        // A moved/reused record and an unloaded slot are classified from their current ranges.
        for (replacement, enabled) in [
            (record([[0, 0, 2], [1, 1, 3]]), 7_u32),
            (CullRecord::default(), 6),
            (record([[0, 0, 9], [1, 1, 10]]), 7),
        ] {
            let changed = [replacement, records[1], records[2]];
            gpu.queue
                .write_buffer(&culler.storage.records, 0, bytemuck::cast_slice(&changed));
            gpu.queue
                .write_buffer(&culler.storage.enabled, 0, bytemuck::bytes_of(&enabled));
            culler.set_history(&[model::HISTORY_RASTER_VISIBLE; 3]);
            clear_depth(&gpu, &target, 0.8);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            raster.encode(
                &mut encoder,
                &target.depth.create_view(&Default::default()),
                culler.slots,
                uniform.viewport,
                (None, None),
            );
            let groups = culler
                .kernels
                .bind_groups(&gpu.device, &culler.storage, None);
            culler
                .kernels
                .encode_cull(&mut encoder, &groups[1], culler.slots, None);
            gpu.queue.submit([encoder.finish()]);
            let expected = reference_args(
                &changed,
                &culler.order,
                CullCamera::new(Some([0.0; 3])),
                INDEX_COUNTS,
                |slot| slot != 0 || changed[0].bounds()[1][2] == 10,
            );
            assert_eq!(
                culler.args(CullPhase::Late),
                expected,
                "edit/unload/reuse at {samples} samples"
            );
        }
        gpu.queue
            .write_buffer(&culler.storage.records, 0, bytemuck::cast_slice(&records));
        gpu.queue
            .write_buffer(&culler.storage.enabled, 0, bytemuck::bytes_of(&7_u32));
    }
}

/// Reads every physical depth sample without resolving coverage or changing its value.
pub(super) fn sampled_depth(gpu: &Gpu, target: &Target) -> Vec<[u32; 4]> {
    let samples = target.samples;
    let size = target.depth.size();
    let texture = if samples == 1 {
        "texture_depth_2d"
    } else {
        "texture_depth_multisampled_2d"
    };
    let load = if samples == 1 {
        "textureLoad(depth, pixel, 0)"
    } else {
        "textureLoad(depth, pixel, i32(sample))"
    };
    let source = format!(
        r#"
@group(0) @binding(0) var depth: {texture};
@group(0) @binding(1) var<storage, read_write> output: array<vec4<u32>>;
@compute @workgroup_size(64)
fn witness(@builtin(global_invocation_id) id: vec3<u32>) {{
    if (id.x >= arrayLength(&output)) {{ return; }}
    let sample = id.x % {samples}u;
    let linear = id.x / {samples}u;
    let pixel = vec2<i32>(i32(linear % {width}u), i32(linear / {width}u));
    output[id.x] = vec4(bitcast<u32>({load}), 0u, 0u, 0u);
}}
"#,
        width = size.width
    );
    let count = (size.width * size.height * samples) as usize;
    let view = target.depth.create_view(&Default::default());
    gpu.compute_readback(
        &source,
        &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
        1,
        count,
        (count as u32).div_ceil(64),
    )
}

#[test]
fn raster_box_ordered_fallback_keeps_each_streams_cpu_order_and_complete_candidates() {
    let Some(gpu) = Gpu::for_fixture("ordered raster fallback") else {
        return;
    };
    let records: Vec<_> = (0..3)
        .map(|index| {
            CullRecord::new(&CullRecordSource {
                base_vertex: index * 4,
                bounds: [[0; 3], [1; 3]],
                cube: (index as u32 * 4)..(index as u32 * 4 + 2),
                solid_ends: [1; 6],
                model: (100 + index as u32)..(101 + index as u32),
                liquid: (200 + index as u32)..(201 + index as u32),
                ..Default::default()
            })
            .unwrap()
        })
        .collect();
    let culler = Culler::new(&gpu, &records, &[7]);
    let cube = [1, 0];
    let models = [0, 1];
    culler.storage.write_stream_order(&gpu.queue, 0, 0, &cube);
    culler.storage.write_stream_order(&gpu.queue, 1, 0, &models);
    culler.set_history(&[0, 1, 3]);
    let mut uniform = input(3);
    uniform.depth[3] = model::OCCLUSION_ORDERED_FALLBACK;
    uniform.order[2] = cube.len() as u32;
    culler
        .storage
        .write_uniform(&gpu.queue, CullPhase::Late, &uniform);
    let groups = culler
        .kernels
        .bind_groups(&gpu.device, &culler.storage, None);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    culler
        .kernels
        .encode_cull(&mut encoder, &groups[1], culler.slots, None);
    gpu.queue.submit([encoder.finish()]);
    let mut expected = reference_args(
        &records,
        &cube,
        CullCamera::new(Some([0.0; 3])),
        INDEX_COUNTS,
        |_| true,
    );
    let other = reference_args(
        &records,
        &models,
        CullCamera::new(Some([0.0; 3])),
        INDEX_COUNTS,
        |_| true,
    );
    for stream in [CullStream::Model, CullStream::Liquid] {
        expected[stream as usize] = other[stream as usize].clone();
    }
    assert_eq!(culler.args(CullPhase::Late), expected);
}

#[test]
fn raster_box_mixed_orders_leave_hidden_solid_commands_zero_on_fixed_submission() {
    let Some(gpu) = Gpu::for_fixture("mixed raster stream orders") else {
        return;
    };
    let records: Vec<_> = (0..2)
        .map(|index| {
            CullRecord::new(&CullRecordSource {
                base_vertex: index * 4,
                bounds: [[0; 3], [1; 3]],
                cube: (index as u32 * 4)..(index as u32 * 4 + 1),
                solid_ends: [1; 6],
                model: if index == 0 { 0..0 } else { 20..21 },
                ..Default::default()
            })
            .unwrap()
        })
        .collect();
    let mut culler = Culler::new(&gpu, &records, &[3]);
    culler.set_order(vec![1, 0]);
    culler.storage.write_stream_order(&gpu.queue, 1, 0, &[0, 1]);
    culler.set_history(&[0, model::HISTORY_RASTER_VISIBLE]);
    culler
        .storage
        .write_uniform(&gpu.queue, CullPhase::Late, &input(2));
    let groups = culler
        .kernels
        .bind_groups(&gpu.device, &culler.storage, None);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.clear_buffer(&culler.storage.args, 0, None);
    culler
        .kernels
        .encode_cull(&mut encoder, &groups[1], culler.slots, None);
    gpu.queue.submit([encoder.finish()]);
    let bytes = read_buffer(&gpu, &culler.storage.args, culler.storage.args.size());
    let words: &[u32] = bytemuck::cast_slice(&bytes);
    let tail =
        args_region(culler.storage.capacity, CullPhase::Late, CullStream::Solid) + ARGS_WORDS;
    assert_eq!(
        words[tail as usize + 1],
        0,
        "a model-only rank cannot emit its hidden cube record into cleared fixed tails"
    );
}

#[test]
fn raster_box_keeps_a_distant_cube_when_only_its_sealed_edge_is_visible() {
    let Some(gpu) = Gpu::for_fixture_with(
        "distant sealed raster bound",
        wgpu::Features::SHADER_EARLY_DEPTH_TEST,
    ) else {
        return;
    };
    if gpu.backend != wgpu::Backend::Vulkan
        || !gpu
            .device
            .features()
            .contains(wgpu::Features::SHADER_EARLY_DEPTH_TEST)
    {
        eprintln!("missing fixture: Vulkan forced early depth for sealed bounds");
        return;
    }
    let mut terrain = Terrain::default();
    terrain.add(
        [0; 3],
        vec![PackedQuad::new([0, 0, 15], Face::PositiveZ, 16, 16, 0)],
        Vec::new(),
    );
    let eye = Vec3::new(12.953125, 11.125, 816.0);
    let mut view = camera(eye, eye + Vec3::NEG_Z);
    view.clip_from_view = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_2,
        1.0,
        0.05,
    );
    view.clip_from_world = view.clip_from_view
        * Mat4::from(
            GlobalTransform::from(Transform::from_translation(eye))
                .affine()
                .inverse(),
        );
    let target = Target::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, 1);
    let edge = 2.0 / SNAPSHOT_SIDE as f32;
    let rectangles = [
        [-1.0, -1.0, 0.8, 0.0, 0.0, 1.0, 0.0, 0.0],
        [edge, -1.0, 0.8, 0.0, 1.0, 1.0, 0.0, 0.0],
        [0.0, -1.0, 0.8, 0.0, edge, -edge, 0.0, 0.0],
        [0.0, 0.0, 0.8, 0.0, edge, 1.0, 0.0, 0.0],
    ];
    hiz::render_scene(
        &gpu,
        &target,
        ("rect_vertex", "colour_fragment"),
        &view.clip_from_world.to_cols_array(),
        &rectangles,
    );
    let before = read_texture(&gpu, &target.color, 0);
    let raster = Raster::through(
        &gpu,
        &terrain,
        &view,
        true,
        wgpu::TextureFormat::Rgba8Unorm,
        "vertex",
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = target.pass(&mut encoder, false);
        raster.draw(
            &mut pass,
            CullStream::Solid,
            &slot_draws(&terrain, 0, eye.as_dvec3().to_array(), CullStream::Solid),
        );
    }
    gpu.queue.submit([encoder.finish()]);
    let after = read_texture(&gpu, &target.color, 0);
    let pixel = ((SNAPSHOT_SIDE / 2 * SNAPSHOT_SIDE + SNAPSHOT_SIDE / 2) * 4) as usize;
    assert_ne!(
        &before[pixel..pixel + 4],
        &after[pixel..pixel + 4],
        "the production cube's sealed edge must reach the open sample"
    );
    let culler = Culler::new(&gpu, &terrain.records, &[1]);
    let mut uniform = CullViewUniform::new(
        &view_input(&view, 0),
        CullPhase::Late,
        1,
        culler.storage.capacity,
        culler.kernels.workgroup,
    );
    uniform.depth[3] = model::OCCLUSION_RASTER;
    culler
        .storage
        .write_uniform(&gpu.queue, CullPhase::Late, &uniform);
    let boxes = box_occlusion::RasterCull::new(
        &gpu.device,
        &culler.storage,
        culler.kernels.workgroup,
        DEPTH_FORMAT,
        1,
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    boxes.encode(
        &mut encoder,
        &target.depth.create_view(&Default::default()),
        1,
        uniform.viewport,
        (None, None),
    );
    gpu.queue.submit([encoder.finish()]);
    let bytes = read_buffer(&gpu, &culler.storage.history, 4);
    assert_ne!(
        bytemuck::cast_slice::<u8, u32>(&bytes)[0] & model::HISTORY_RASTER_VISIBLE,
        0,
        "a section with a visible sealed sample cannot be hidden"
    );
}
