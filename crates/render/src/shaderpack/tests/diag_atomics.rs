//! TEMPORARY DIAGNOSTIC: loops image atomics under extra GPU load and counts stale reads.

use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// Counts its 16 invocations into an `r32ui` custom image with an image atomic.
const COUNT_ATOMICALLY: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    imageAtomicAdd(counter, ivec2(0), 1u);
}
";

/// Writes the counter's first texel, over 255, to red.
const SHOW_COUNT: &str = "#version 430
uniform usampler2D counterSampler;
void main() {
/* DRAWBUFFERS:0 */
    uint count = texelFetch(counterSampler, ivec2(0), 0).r;
    gl_FragData[0] = vec4(float(count) / 255.0, 0.0, 0.0, 1.0);
}
";

const STORE_SIXTEEN: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    if (gl_LocalInvocationIndex == 0u) {
        imageStore(counter, ivec2(0), uvec4(16u));
    }
}
";

const ONE_ATOMIC: &str = "#version 430
layout(local_size_x = 1, local_size_y = 1) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    imageAtomicAdd(counter, ivec2(0), 16u);
}
";

/// Adds 1 from each of the 4x4 pixels at the bottom-left corner.
const FRAGMENT_ATOMICS: &str = "#version 430
layout(r32ui) uniform uimage2D counter;
void main() {
/* DRAWBUFFERS:1 */
    if (all(lessThan(ivec2(gl_FragCoord.xy), ivec2(4)))) {
        imageAtomicAdd(counter, ivec2(0), 1u);
    }
    gl_FragData[0] = vec4(0.0);
}
";

const CLEARED: &str =
    "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT true false 1 1\n";
const KEPT: &str =
    "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT false false 1 1\n";

/// Threads running [`load`]; `DIAG_LOAD` overrides the default of 2.
fn load_threads() -> usize {
    std::env::var("DIAG_LOAD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2)
}

/// Keeps the GPU busy from its own device until `stop` is set.
fn load(stop: &AtomicBool) {
    let gpu = gpu().expect("adapter");
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var<storage, read_write> out: array<u32>;
                @compute @workgroup_size(64) fn spin(@builtin(global_invocation_id) id: vec3<u32>) {
                    var x = id.x;
                    for (var i = 0u; i < 20000u; i++) { x = x * 1664525u + 1013904223u; }
                    out[id.x] = x;
                }"
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("spin"),
            compilation_options: Default::default(),
            cache: None,
        });
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4 * 64 * 256,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    while !stop.load(Ordering::Relaxed) {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(256, 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
    }
}

const SHADERS: &str = "
@group(0) @binding(0) var counter: texture_storage_2d<r32uint, atomic>;
@compute @workgroup_size(1) fn add(@builtin(global_invocation_id) id: vec3<u32>) {
    textureAtomicAdd(counter, vec2<i32>(id.xy), 1u);
}
@compute @workgroup_size(1) fn add_zero() {
    textureAtomicAdd(counter, vec2<i32>(0), 1u);
}

@group(0) @binding(0) var rw: texture_storage_2d<r32uint, read_write>;
@compute @workgroup_size(1) fn touch_one() {
    let v = textureLoad(rw, vec2<i32>(0));
    if (v.r == 0xffffffffu) { textureStore(rw, vec2<i32>(0), v); }
}

@group(0) @binding(0) var sampled: texture_2d<u32>;
@vertex fn fullscreen(@builtin(vertex_index) v: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((v << 1u) & 2u), f32(v & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn show(@builtin(position) p: vec4<f32>) -> @location(0) vec4<u32> {
    return vec4<u32>(textureLoad(sampled, vec2<i32>(p.xy), 0).r, 0u, 0u, 0u);
}
@fragment fn frag_add(@builtin(position) p: vec4<f32>) -> @location(0) vec4<u32> {
    textureAtomicAdd(counter, vec2<i32>(p.xy), 1u);
    return vec4<u32>(0u);
}
@fragment fn frag_add_zero() -> @location(0) vec4<u32> {
    textureAtomicAdd(counter, vec2<i32>(0), 1u);
    return vec4<u32>(0u);
}
";

/// Which stage performs the atomics, and whether every invocation adds 1 to its own texel or to
/// texel (0, 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Writer {
    ComputeOwn,
    ComputeZero,
    FragmentOwn,
    FragmentZero,
}

/// What runs between the atomics and the render pass that reads the texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Between {
    Nothing,
    /// A one-invocation compute pass that loads texel (0, 0) through a read-write binding and
    /// stores it back only if it is `u32::MAX`.
    TouchOne,
}

struct Pipelines {
    add: wgpu::ComputePipeline,
    add_zero: wgpu::ComputePipeline,
    touch_one: wgpu::ComputePipeline,
    show: wgpu::RenderPipeline,
    frag_add: wgpu::RenderPipeline,
    frag_add_zero: wgpu::RenderPipeline,
}

fn pipelines(device: &wgpu::Device) -> Pipelines {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(SHADERS.into()),
    });
    let compute = |entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let render = |entry| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry),
                targets: &[Some(wgpu::TextureFormat::R32Uint.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    Pipelines {
        add: compute("add"),
        add_zero: compute("add_zero"),
        touch_one: compute("touch_one"),
        show: render("show"),
        frag_add: render("frag_add"),
        frag_add_zero: render("frag_add_zero"),
    }
}

fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }],
    })
}

/// Records a full-target draw with `pipeline` and `bind` into `target`.
fn draw(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::Texture,
    pipeline: &wgpu::RenderPipeline,
    bind: &wgpu::BindGroup,
) {
    let target_view = target.create_view(&Default::default());
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    pass.draw(0..3, 0..1);
}

/// One trial: zeroes a fresh `side`×`side` `r32uint` texture, adds 1 per invocation with
/// atomics as `writer` says, runs `between`, reads it in a render pass and returns how many
/// texels read a wrong value.
fn trial(gpu: &GpuContext, pipes: &Pipelines, side: u32, writer: Writer, between: Between) -> u32 {
    let device = &gpu.device;
    let extent = wgpu::Extent3d {
        width: side,
        height: side,
        depth_or_array_layers: 1,
    };
    let texture = |usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Uint,
            usage,
            view_formats: &[],
        })
    };
    let counter = texture(
        wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::STORAGE_ATOMIC,
    );
    gpu.queue.write_texture(
        counter.as_image_copy(),
        &vec![0; (4 * side * side) as usize],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * side),
            rows_per_image: None,
        },
        extent,
    );
    let view = counter.create_view(&Default::default());
    let scratch = texture(wgpu::TextureUsages::RENDER_ATTACHMENT);
    let target = texture(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(256 * side),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    match writer {
        Writer::ComputeOwn | Writer::ComputeZero => {
            let pipeline = if writer == Writer::ComputeOwn {
                &pipes.add
            } else {
                &pipes.add_zero
            };
            let add_group = group(device, &pipeline.get_bind_group_layout(0), &view);
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &add_group, &[]);
            pass.dispatch_workgroups(side, side, 1);
        }
        Writer::FragmentOwn | Writer::FragmentZero => {
            let pipeline = if writer == Writer::FragmentOwn {
                &pipes.frag_add
            } else {
                &pipes.frag_add_zero
            };
            let add_group = group(device, &pipeline.get_bind_group_layout(0), &view);
            draw(&mut encoder, &scratch, pipeline, &add_group);
        }
    }
    if between == Between::TouchOne {
        let touch_group = group(device, &pipes.touch_one.get_bind_group_layout(0), &view);
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipes.touch_one);
        pass.set_bind_group(0, &touch_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    let show_group = group(device, &pipes.show.get_bind_group_layout(0), &view);
    draw(&mut encoder, &target, &pipes.show, &show_group);
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        extent,
    );
    gpu.queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = readback.slice(..).get_mapped_range().unwrap().to_vec();
    let mut wrong = 0;
    for y in 0..side {
        for x in 0..side {
            let at = (y * 256 + x * 4) as usize;
            let value = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            let expected = match writer {
                Writer::ComputeOwn | Writer::FragmentOwn => 1,
                Writer::ComputeZero | Writer::FragmentZero if x == 0 && y == 0 => side * side,
                Writer::ComputeZero | Writer::FragmentZero => 0,
            };
            wrong += u32::from(value != expected);
        }
    }
    wrong
}

/// TEMPORARY DIAGNOSTIC: how texture size and the atomic pattern affect stale reads, and
/// whether a touch pass fixes them.
#[test]
fn diag_raw_wgpu_image_atomics() {
    let Some(probe) = gpu() else {
        return;
    };
    if !probe
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ATOMIC)
    {
        return;
    }
    let adapter = format!("{:?}", probe.adapter.get_info());
    drop(probe);
    const THREADS: usize = 4;
    const RUNS: usize = 25;
    const ROUNDS: usize = 4;
    let mut conditions = Vec::new();
    for side in [1, 8, 64] {
        for writer in [
            Writer::ComputeOwn,
            Writer::ComputeZero,
            Writer::FragmentOwn,
            Writer::FragmentZero,
        ] {
            if side == 1 && matches!(writer, Writer::ComputeZero | Writer::FragmentZero) {
                continue;
            }
            for between in [Between::Nothing, Between::TouchOne] {
                conditions.push((side, writer, between));
            }
        }
    }
    // (trials with any wrong texel, wrong texels)
    let mut totals: BTreeMap<(u32, Writer, Between), (u32, u64)> = BTreeMap::new();
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let loaders: Vec<_> = (0..load_threads())
            .map(|_| scope.spawn(|| load(&stop)))
            .collect();
        let devices: Vec<_> = (0..THREADS)
            .map(|_| {
                let gpu = gpu().expect("adapter");
                let pipes = pipelines(&gpu.device);
                (gpu, pipes)
            })
            .collect();
        for _ in 0..ROUNDS {
            for &(side, writer, between) in &conditions {
                let seen: Vec<(u32, u64)> = std::thread::scope(|inner| {
                    let workers: Vec<_> = devices
                        .iter()
                        .map(|(gpu, pipes)| {
                            inner.spawn(move || {
                                let mut seen = (0, 0);
                                for _ in 0..RUNS {
                                    let wrong = trial(gpu, pipes, side, writer, between);
                                    seen.0 += u32::from(wrong != 0);
                                    seen.1 += u64::from(wrong);
                                }
                                seen
                            })
                        })
                        .collect();
                    workers.into_iter().map(|w| w.join().unwrap()).collect()
                });
                let total = totals.entry((side, writer, between)).or_default();
                for (trials, texels) in seen {
                    total.0 += trials;
                    total.1 += texels;
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        for loader in loaders {
            loader.join().unwrap();
        }
    });
    let mut report = format!("adapter: {adapter}\n");
    for ((side, writer, between), (trials, texels)) in &totals {
        report += &format!(
            "touch {side}x{side} {writer:?} {between:?}: {trials} / {} trials wrong, {texels} texels wrong\n",
            THREADS * RUNS * ROUNDS
        );
    }
    panic!("DIAG\n{report}");
}

/// TEMPORARY DIAGNOSTIC: counts how often each shader pack counter variant reads a wrong value.
#[test]
fn diag_image_atomics_flake_rate() {
    let Some(probe) = gpu() else {
        return;
    };
    if !probe
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ATOMIC)
    {
        return;
    }
    drop(probe);
    const THREADS: usize = 4;
    const RUNS: usize = 25;
    const ROUNDS: usize = 4;
    let compute = |properties, program| {
        vec![
            ("shaders.properties", properties),
            ("composite.csh", program),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", SHOW_COUNT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ]
    };
    let fragment = vec![
        ("shaders.properties", CLEARED),
        ("composite.vsh", PASS_VERTEX),
        ("composite.fsh", FRAGMENT_ATOMICS),
        ("composite1.vsh", PASS_VERTEX),
        ("composite1.fsh", SHOW_COUNT),
        ("final.vsh", PASS_VERTEX),
        ("final.fsh", FINAL),
    ];
    // (name, files, frames, expected red)
    type Files<'a> = Vec<(&'a str, &'a str)>;
    let variants: Vec<(&str, Files, usize, u8)> = vec![
        (
            "A atomic, cleared, 2 frames (the CI test)",
            compute(CLEARED, COUNT_ATOMICALLY),
            2,
            16,
        ),
        (
            "B imageStore, cleared, 2 frames",
            compute(CLEARED, STORE_SIXTEEN),
            2,
            16,
        ),
        (
            "C atomic, kept, 1 frame",
            compute(KEPT, COUNT_ATOMICALLY),
            1,
            16,
        ),
        (
            "M one invocation adds 16, kept, 1 frame",
            compute(KEPT, ONE_ATOMIC),
            1,
            16,
        ),
        ("F fragment atomics, cleared, 2 frames", fragment, 2, 16),
    ];
    let mut totals: BTreeMap<usize, BTreeMap<u8, u32>> = BTreeMap::new();
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let loaders: Vec<_> = (0..load_threads())
            .map(|_| scope.spawn(|| load(&stop)))
            .collect();
        let devices: Vec<_> = (0..THREADS).map(|_| gpu().expect("adapter")).collect();
        for _ in 0..ROUNDS {
            for (index, (_, files, frames, _)) in variants.iter().enumerate() {
                let seen: Vec<BTreeMap<u8, u32>> = std::thread::scope(|inner| {
                    let workers: Vec<_> = devices
                        .iter()
                        .map(|gpu| {
                            inner.spawn(move || {
                                let mut seen = BTreeMap::<u8, u32>::new();
                                for _ in 0..RUNS {
                                    let (_, bytes) = render_frames(gpu, files, *frames);
                                    *seen.entry(bytes[0]).or_default() += 1;
                                }
                                seen
                            })
                        })
                        .collect();
                    workers.into_iter().map(|w| w.join().unwrap()).collect()
                });
                let total = totals.entry(index).or_default();
                for map in seen {
                    for (value, count) in map {
                        *total.entry(value).or_default() += count;
                    }
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        for loader in loaders {
            loader.join().unwrap();
        }
    });
    let mut report = String::new();
    for (index, total) in &totals {
        let (name, .., expected) = &variants[*index];
        let wrong: u32 = total
            .iter()
            .filter(|(value, _)| value.abs_diff(*expected) > 1)
            .map(|(_, count)| count)
            .sum();
        report += &format!(
            "pack {name}: {wrong} / {} wrong, expected {expected}, seen {total:?}\n",
            THREADS * RUNS * ROUNDS
        );
    }
    panic!("DIAG\n{report}");
}
