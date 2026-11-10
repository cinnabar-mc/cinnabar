//! The two-phase GPU cull draws the CPU cull's pixels from any stale history.

use super::*;

/// How the fixture consumes compacted commands: as production does, or resolved on the CPU
/// when the device cannot draw indirectly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Submission {
    Fixed,
    Count,
    Resolved,
}

/// Draws the CPU path and the two-phase GPU path, from a stale history, through `vertex` and
/// every submission the device offers, and fails when more than `tolerance` pixels differ.
fn compare_cull_paths(vertex: &str, tolerance: usize, boxes: bool, samples: u32) {
    let fixed = wgpu::Features::INDIRECT_FIRST_INSTANCE;
    let count = fixed | wgpu::Features::MULTI_DRAW_INDIRECT_COUNT;
    let Some((gpu, adapter)) = Gpu::for_fixture_with_adapter(
        "gpu culled terrain raster",
        count
            | wgpu::Features::SHADER_EARLY_DEPTH_TEST
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        wgpu::Limits::default(),
    ) else {
        return;
    };
    let features = gpu.device.features();
    if [DEPTH_FORMAT, wgpu::TextureFormat::Rgba8Unorm]
        .into_iter()
        .any(|format| {
            !adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(samples)
        })
    {
        eprintln!("missing fixture: {samples}x terrain raster samples");
        return;
    }
    if boxes
        && (gpu.backend != wgpu::Backend::Vulkan
            || !features.contains(wgpu::Features::SHADER_EARLY_DEPTH_TEST))
    {
        eprintln!("missing fixture: Vulkan forced early depth for box colour comparison");
        return;
    }
    let mut submissions = [
        (Submission::Fixed, features.contains(fixed)),
        (Submission::Count, features.contains(count)),
    ]
    .into_iter()
    .filter_map(|(submission, offered)| offered.then_some(submission))
    .collect::<Vec<_>>();
    if submissions.is_empty() {
        submissions.push(Submission::Resolved);
    }
    eprintln!("gpu cull raster on {:?}: {submissions:?}", gpu.backend);
    let terrain = terrain();
    let slots = terrain.records.len();
    let enabled = enabled_words(|slot| slot != 7, slots);
    let mut culler = Culler::new(&gpu, &terrain.records, &enabled);
    let mut random = Lcg(3);
    let mut history = (0..slots)
        .filter(|_| random.next(2) == 0)
        .collect::<BTreeSet<_>>();
    let cameras = [
        camera(Vec3::new(8.25, 72.5, 8.75), Vec3::new(10.0, 70.0, -40.0)),
        camera(Vec3::new(20.5, 75.0, 2.0), Vec3::new(0.0, 68.0, -60.0)),
        camera(Vec3::new(-30.0, 90.0, -8.0), Vec3::new(10.0, 60.0, -70.0)),
    ];
    let mut occluded_any = false;
    for (index, camera) in cameras.iter().enumerate() {
        let raster = Raster::through_samples(
            &gpu,
            &terrain,
            camera,
            true,
            wgpu::TextureFormat::Rgba8Unorm,
            vertex,
            samples,
        );
        let eye = camera.eye.as_dvec3().to_array();
        let visible = (0..slots)
            .filter(|&slot| {
                model::slot_enabled(&enabled, slot)
                    && bevy_visible(&camera.frustum, terrain.chunks[slot].0)
            })
            .collect::<Vec<_>>();
        let mut front_to_back = visible.clone();
        let depth = |slot: usize| {
            (Vec3::from_array(terrain.chunks[slot].0.map(|v| v as f32)) - camera.eye).length()
        };
        front_to_back.sort_by(|&a, &b| depth(a).total_cmp(&depth(b)));
        // Production visits slots nearest first too.
        let mut nearest_first = (0..slots as u32).collect::<Vec<_>>();
        nearest_first.sort_by(|&a, &b| depth(a as usize).total_cmp(&depth(b as usize)));
        culler.set_order(nearest_first);

        // CPU path: facing solid runs front to back, then the cutout tails.
        let cpu = Target::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, samples);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let mut cpu_draws = 0;
        {
            let mut pass = cpu.pass(&mut encoder, true);
            for (stream, draws) in [CullStream::Solid, CullStream::Cutout].map(|stream| {
                let draws = front_to_back
                    .iter()
                    .flat_map(|&slot| slot_draws(&terrain, slot, eye, stream))
                    .collect::<Vec<_>>();
                (stream, draws)
            }) {
                cpu_draws += draws.len();
                raster.draw(&mut pass, stream, &draws);
            }
        }
        gpu.queue.submit([encoder.finish()]);
        let expected = read_texture(&gpu, cpu.resolved.as_ref().unwrap_or(&cpu.color), 0);
        gpu_snapshot::save(&format!("gpu_cull_cpu_{vertex}_{index}"), &expected);
        let background = &expected[..4];
        assert!(
            expected
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel != background),
            "camera {index} sees terrain"
        );

        // GPU path: early from the stale history, Hi-Z from that depth, then late.
        let mut next_history = None;
        for &submission in &submissions {
            let history_words = bits_of(&history, slots)
                .into_iter()
                .map(|word| {
                    if boxes {
                        word * model::HISTORY_RASTER_VISIBLE
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>();
            culler.set_history(&history_words);
            let gpu_target = Target::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, samples);
            let mut gpu_draws = 0;
            let mut pyramid_mips = 0;
            for phase in CullPhase::ALL {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                if submission == Submission::Fixed && phase == CullPhase::Early {
                    encoder.clear_buffer(&culler.storage.args, 0, None);
                }
                let pyramid = (phase == CullPhase::Late && !boxes)
                    .then(|| gpu_target.pyramid(&gpu, &culler.kernels, &mut encoder));
                if let Some(pyramid) = &pyramid {
                    pyramid_mips = pyramid.mip_count();
                }
                let input = view_input(camera, pyramid_mips);
                if boxes {
                    let mut uniform = CullViewUniform::new(
                        &input,
                        phase,
                        culler.slots,
                        culler.storage.capacity,
                        culler.kernels.workgroup,
                    );
                    uniform.depth[3] = model::OCCLUSION_RASTER;
                    culler.storage.write_uniform(&gpu.queue, phase, &uniform);
                    if phase == CullPhase::Late {
                        let boxes = box_occlusion::RasterCull::new(
                            &gpu.device,
                            &culler.storage,
                            culler.kernels.workgroup,
                            DEPTH_FORMAT,
                            samples,
                        );
                        boxes.encode(
                            &mut encoder,
                            &gpu_target.depth.create_view(&Default::default()),
                            culler.slots,
                            uniform.viewport,
                            (None, None),
                        );
                    }
                    let groups = culler
                        .kernels
                        .bind_groups(&gpu.device, &culler.storage, None);
                    culler.kernels.encode_cull(
                        &mut encoder,
                        &groups[phase as usize],
                        culler.slots,
                        None,
                    );
                } else {
                    culler.encode(&mut encoder, &input, phase, pyramid.as_ref());
                }
                if submission == Submission::Resolved {
                    gpu.queue.submit([encoder.finish()]);
                    encoder = gpu.device.create_command_encoder(&Default::default());
                    let args = culler.args(phase);
                    let mut pass = gpu_target.pass(&mut encoder, phase == CullPhase::Early);
                    for stream in [CullStream::Solid, CullStream::Cutout] {
                        raster.draw(&mut pass, stream, &args[stream as usize]);
                    }
                } else {
                    let mut pass = gpu_target.pass(&mut encoder, phase == CullPhase::Early);
                    for stream in [CullStream::Solid, CullStream::Cutout] {
                        if boxes && phase == CullPhase::Early {
                            if stream != CullStream::Solid {
                                continue;
                            }
                            raster.bind_early(&mut pass, &culler.storage.draw_offsets);
                        } else {
                            raster.bind(&mut pass, stream, &culler.storage.draw_offsets);
                        }
                        let offset = u64::from(args_region(culler.storage.capacity, phase, stream));
                        let max = slots as u32 * stream.draws_per_record();
                        if submission == Submission::Count {
                            pass.multi_draw_indexed_indirect_count(
                                &culler.storage.args,
                                offset * 4,
                                &culler.storage.draw_counts,
                                u64::from(count_index(phase, stream)) * 4,
                                max,
                            );
                        } else {
                            pass.multi_draw_indexed_indirect(&culler.storage.args, offset * 4, max);
                        }
                    }
                }
                gpu.queue.submit([encoder.finish()]);
                gpu_draws += culler.args(phase).iter().map(Vec::len).sum::<usize>();
            }
            let next = if boxes {
                let bytes = read_buffer(
                    &gpu,
                    &culler.storage.record_draws,
                    u64::from(culler.slots) * 4,
                );
                bytemuck::cast_slice::<u8, u32>(&bytes)
                    .iter()
                    .enumerate()
                    .filter_map(|(rank, word)| {
                        ((word & model::DECISION_CANDIDATE != 0)
                            && (word & model::DECISION_HIDDEN == 0))
                            .then_some(culler.order[rank] as usize)
                    })
                    .collect()
            } else {
                culler.history()
            };
            assert!(next.iter().all(|slot| visible.contains(slot)));
            assert!(
                next_history
                    .as_ref()
                    .is_none_or(|previous| *previous == next)
            );

            let actual = read_texture(
                &gpu,
                gpu_target.resolved.as_ref().unwrap_or(&gpu_target.color),
                0,
            );
            if boxes {
                let actual_depth = box_occlusion_tests::sampled_depth(&gpu, &gpu_target);
                let expected_depth = box_occlusion_tests::sampled_depth(&gpu, &cpu);
                let different = actual_depth
                    .iter()
                    .zip(&expected_depth)
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(different, 0, "camera {index} depth differs");
            }
            gpu_snapshot::save(
                &format!("gpu_cull_gpu_{vertex}_{index}_{submission:?}"),
                &actual,
            );
            let mismatched = expected
                .as_chunks::<4>()
                .0
                .iter()
                .zip(actual.as_chunks::<4>().0.iter())
                .filter(|(a, b)| a != b)
                .count();
            assert!(
                mismatched <= tolerance,
                "camera {index} through {vertex} by {submission:?}: {mismatched} pixels differ"
            );
            eprintln!(
                "gpu cull camera {index} through {vertex} by {submission:?}: {mismatched} pixels differ; cpu path {cpu_draws} draws over {} sub-chunks, gpu path {gpu_draws} draws ({} sub-chunks pass Hi-Z)",
                visible.len(),
                next.len()
            );
            next_history = Some(next);
        }
        let next = next_history.expect("one submission ran");
        occluded_any |= next.len() < visible.len();
        history = next;
    }
    assert!(occluded_any, "the wall must occlude some sub-chunks");
}

#[test]
fn gpu_culled_terrain_rasterises_exactly_like_the_cpu_culled_path() {
    compare_cull_paths("unsealed_vertex", 0, false, 1);
}

/// Sealed quads overlap coplanar neighbours by a sub-pixel sliver, where the
/// paths' different draw orders may pick either neighbour's edge texel.
#[test]
fn gpu_culled_sealed_terrain_rasterises_like_the_cpu_culled_path() {
    compare_cull_paths("vertex", SEALED_DRAW_ORDER_PIXELS, false, 1);
}

/// Seam pixels per 256 x 256 frame whose colour may follow draw order.
const SEALED_DRAW_ORDER_PIXELS: usize = 24;

/// Current-frame boxes must retain exact completed colour and depth after camera changes.
#[test]
fn raster_box_occlusion_matches_unculled_terrain_on_first_disocclusion_frame() {
    for samples in [1, 2, 4, 8] {
        compare_cull_paths("vertex", 0, true, samples);
    }
}
