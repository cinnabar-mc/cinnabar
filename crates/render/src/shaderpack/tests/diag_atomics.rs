//! TEMPORARY DIAGNOSTIC: loops image atomic variants on Metal, with and without the atomic
//! texture fence, interleaved in one process under extra GPU load.

use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// TEMPORARY DIAGNOSTIC: counts how often each counter variant reads a wrong value with and
/// without the fence, interleaving the conditions so runner state affects both alike.
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
    let adapter = format!("{:?}", probe.adapter.get_info());
    drop(probe);
    const THREADS: usize = 4;
    const RUNS: usize = 25;
    const ROUNDS: usize = 4;
    // (name, properties, compute, frames, expected red)
    let variants: [(&str, &str, &str, usize, u8); 4] = [
        (
            "A atomic, cleared, 2 frames",
            CLEARED,
            COUNT_ATOMICALLY,
            2,
            16,
        ),
        (
            "B imageStore, cleared, 2 frames",
            CLEARED,
            STORE_SIXTEEN,
            2,
            16,
        ),
        ("C atomic, kept, 1 frame", KEPT, COUNT_ATOMICALLY, 1, 16),
        (
            "M one invocation adds 16, kept, 1 frame",
            KEPT,
            ONE_ATOMIC,
            1,
            16,
        ),
    ];
    let mut totals: BTreeMap<(usize, bool), BTreeMap<u8, u32>> = BTreeMap::new();
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let loaders: Vec<_> = (0..load_threads())
            .map(|_| scope.spawn(|| load(&stop)))
            .collect();
        for _ in 0..ROUNDS {
            for fence in [false, true] {
                // SAFETY: only this test's threads read the environment, and none are reading
                // it while it changes; devices read it when they open.
                unsafe {
                    if fence {
                        std::env::remove_var("WGPU_METAL_NO_ATOMIC_FENCE");
                    } else {
                        std::env::set_var("WGPU_METAL_NO_ATOMIC_FENCE", "1");
                    }
                }
                let devices: Vec<_> = (0..THREADS).map(|_| gpu().expect("adapter")).collect();
                for (index, (_, properties, compute, frames, _)) in variants.iter().enumerate() {
                    let seen: Vec<BTreeMap<u8, u32>> = std::thread::scope(|inner| {
                        let workers: Vec<_> = devices
                            .iter()
                            .map(|gpu| {
                                inner.spawn(move || {
                                    let mut seen = BTreeMap::<u8, u32>::new();
                                    for _ in 0..RUNS {
                                        let (_, bytes) = render_frames(
                                            gpu,
                                            &[
                                                ("shaders.properties", properties),
                                                ("composite.csh", compute),
                                                ("composite.vsh", PASS_VERTEX),
                                                ("composite.fsh", SHOW_COUNT),
                                                ("final.vsh", PASS_VERTEX),
                                                ("final.fsh", FINAL),
                                            ],
                                            *frames,
                                        );
                                        *seen.entry(bytes[0]).or_default() += 1;
                                    }
                                    seen
                                })
                            })
                            .collect();
                        workers.into_iter().map(|w| w.join().unwrap()).collect()
                    });
                    let total = totals.entry((index, fence)).or_default();
                    for map in seen {
                        for (value, count) in map {
                            *total.entry(value).or_default() += count;
                        }
                    }
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        for loader in loaders {
            loader.join().unwrap();
        }
    });
    let mut report = format!("adapter: {adapter}\n");
    for ((index, fence), total) in &totals {
        let (name, .., expected) = variants[*index];
        let wrong: u32 = total
            .iter()
            .filter(|(value, _)| value.abs_diff(expected) > 1)
            .map(|(_, count)| count)
            .sum();
        report += &format!(
            "{name} [fence {fence}]: {wrong} / {} wrong, expected {expected}, seen {total:?}\n",
            THREADS * RUNS * ROUNDS
        );
    }
    panic!("DIAG\n{report}");
}

const RAW_SHADERS: &str = "
@group(0) @binding(0) var counter: texture_storage_2d<r32uint, atomic>;
@compute @workgroup_size(4, 4) fn add() { textureAtomicAdd(counter, vec2<i32>(0), 1u); }

@group(0) @binding(0) var stored: texture_storage_2d<r32uint, write>;
@compute @workgroup_size(4, 4) fn store(@builtin(local_invocation_index) i: u32) {
    if (i == 0u) { textureStore(stored, vec2<i32>(0), vec4<u32>(16u)); }
}

@group(0) @binding(0) var sampled: texture_2d<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1) fn read() { out[0] = textureLoad(sampled, vec2<i32>(0), 0).r; }

@vertex fn fullscreen(@builtin(vertex_index) v: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((v << 1u) & 2u), f32(v & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn show() -> @location(0) vec4<u32> {
    return vec4<u32>(textureLoad(sampled, vec2<i32>(0), 0).r, 0u, 0u, 0u);
}
";

/// How a raw wgpu trial reads the counter after the compute pass writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RawRead {
    /// A fragment shader loads it in a render pass in the same command buffer.
    Render,
    /// A compute shader loads it in a later compute pass in the same command buffer.
    Compute,
    /// `copy_texture_to_buffer` in the same command buffer.
    Copy,
    /// A render pass in a second submission.
    RenderNextSubmit,
    /// A render pass after the CPU waited for the compute submission.
    RenderAfterWait,
}

struct RawPipelines {
    add: wgpu::ComputePipeline,
    store: wgpu::ComputePipeline,
    read: wgpu::ComputePipeline,
    show: wgpu::RenderPipeline,
}

fn raw_pipelines(device: &wgpu::Device) -> RawPipelines {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(RAW_SHADERS.into()),
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
    RawPipelines {
        add: compute("add"),
        store: compute("store"),
        read: compute("read"),
        show: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                entry_point: Some("show"),
                targets: &[Some(wgpu::TextureFormat::R32Uint.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        }),
    }
}

/// One trial: zeroes a fresh 1×1 `r32uint` texture, adds 16 to it with atomics (or stores 16),
/// reads it back as `read` says and returns the value read.
fn raw_trial(gpu: &GpuContext, pipelines: &RawPipelines, atomic: bool, read: RawRead) -> u32 {
    let device = &gpu.device;
    let extent = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let counter = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Uint,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | if atomic {
                wgpu::TextureUsages::STORAGE_ATOMIC
            } else {
                wgpu::TextureUsages::empty()
            },
        view_formats: &[],
    });
    gpu.queue.write_texture(
        counter.as_image_copy(),
        &[0; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        extent,
    );
    let view = counter.create_view(&Default::default());
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Uint,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let writer = if atomic {
        &pipelines.add
    } else {
        &pipelines.store
    };
    let write_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &writer.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(writer);
        pass.set_bind_group(0, &write_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    if matches!(read, RawRead::RenderNextSubmit | RawRead::RenderAfterWait) {
        gpu.queue.submit([encoder.finish()]);
        if read == RawRead::RenderAfterWait {
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        encoder = device.create_command_encoder(&Default::default());
    }
    let copy_to_readback = |encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture| {
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
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
    };
    match read {
        RawRead::Copy => copy_to_readback(&mut encoder, &counter),
        RawRead::Compute => {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipelines.read.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: out.as_entire_binding(),
                    },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipelines.read);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            encoder.copy_buffer_to_buffer(&out, 0, &readback, 0, 4);
        }
        RawRead::Render | RawRead::RenderNextSubmit | RawRead::RenderAfterWait => {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipelines.show.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
            });
            {
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
                pass.set_pipeline(&pipelines.show);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            copy_to_readback(&mut encoder, &target);
        }
    }
    gpu.queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = readback.slice(..4).get_mapped_range().unwrap().to_vec();
    u32::from_le_bytes(bytes.try_into().unwrap())
}

/// TEMPORARY DIAGNOSTIC: the same counter in plain wgpu, without the shader pack pipeline,
/// read back in several ways under GPU load.
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
    drop(probe);
    const THREADS: usize = 4;
    const RUNS: usize = 50;
    const ROUNDS: usize = 4;
    let conditions = [
        (true, RawRead::Render),
        (true, RawRead::Compute),
        (true, RawRead::Copy),
        (true, RawRead::RenderNextSubmit),
        (true, RawRead::RenderAfterWait),
        (false, RawRead::Render),
    ];
    let mut totals: BTreeMap<(bool, RawRead), BTreeMap<u32, u32>> = BTreeMap::new();
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let loaders: Vec<_> = (0..load_threads())
            .map(|_| scope.spawn(|| load(&stop)))
            .collect();
        let devices: Vec<_> = (0..THREADS)
            .map(|_| {
                let gpu = gpu().expect("adapter");
                let pipelines = raw_pipelines(&gpu.device);
                (gpu, pipelines)
            })
            .collect();
        for _ in 0..ROUNDS {
            for &(atomic, read) in &conditions {
                let seen: Vec<BTreeMap<u32, u32>> = std::thread::scope(|inner| {
                    let workers: Vec<_> = devices
                        .iter()
                        .map(|(gpu, pipelines)| {
                            inner.spawn(move || {
                                let mut seen = BTreeMap::new();
                                for _ in 0..RUNS {
                                    *seen
                                        .entry(raw_trial(gpu, pipelines, atomic, read))
                                        .or_default() += 1;
                                }
                                seen
                            })
                        })
                        .collect();
                    workers.into_iter().map(|w| w.join().unwrap()).collect()
                });
                let total = totals.entry((atomic, read)).or_default();
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
    for ((atomic, read), total) in &totals {
        let wrong: u32 = total
            .iter()
            .filter(|(value, _)| **value != 16)
            .map(|(_, count)| count)
            .sum();
        report += &format!(
            "raw {} {read:?}: {wrong} / {} wrong, seen {total:?}\n",
            if *atomic { "atomic" } else { "store" },
            THREADS * RUNS * ROUNDS
        );
    }
    panic!("DIAG\n{report}");
}
