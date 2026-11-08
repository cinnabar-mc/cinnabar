//! TEMPORARY DIAGNOSTIC: finds which follow-up pass makes texture atomics visible to later
//! render passes on Metal, under extra GPU load.

use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

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

const SIDE: u32 = 64;

const SHADERS: &str = "
@group(0) @binding(0) var counter: texture_storage_2d<r32uint, atomic>;
@compute @workgroup_size(8, 8) fn add(@builtin(global_invocation_id) id: vec3<u32>) {
    textureAtomicAdd(counter, vec2<i32>(id.xy), 1u);
}

@group(0) @binding(0) var rw: texture_storage_2d<r32uint, read_write>;
@compute @workgroup_size(1) fn touch_one() {
    let v = textureLoad(rw, vec2<i32>(0));
    if (v.r == 0xffffffffu) { textureStore(rw, vec2<i32>(0), v); }
}
@compute @workgroup_size(8, 8) fn touch_all(@builtin(global_invocation_id) id: vec3<u32>) {
    let v = textureLoad(rw, vec2<i32>(id.xy));
    if (v.r == 0xffffffffu) { textureStore(rw, vec2<i32>(id.xy), v); }
}
@compute @workgroup_size(1) fn touch_dims() {
    if (textureDimensions(rw).x == 0u) { textureStore(rw, vec2<i32>(0), vec4<u32>(0u)); }
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
";

/// Which shader stage performs the atomics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Writer {
    Compute,
    Fragment,
}

/// What runs between the atomics and the render pass that reads the texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Between {
    /// Nothing.
    Nothing,
    /// A one-invocation compute pass that loads texel (0, 0) through a read-write binding and
    /// stores it back only if it is `u32::MAX`.
    TouchOne,
    /// The same for every texel.
    TouchAll,
    /// A one-invocation compute pass that binds the texture read-write and stores only if its
    /// width is zero, without loading it.
    TouchDims,
    /// [`Self::TouchOne`] as a second dispatch inside the compute pass that does the atomics.
    TouchOneSamePass,
}

struct Pipelines {
    add: wgpu::ComputePipeline,
    touch_one: wgpu::ComputePipeline,
    touch_all: wgpu::ComputePipeline,
    touch_dims: wgpu::ComputePipeline,
    show: wgpu::RenderPipeline,
    frag_add: wgpu::RenderPipeline,
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
        touch_one: compute("touch_one"),
        touch_all: compute("touch_all"),
        touch_dims: compute("touch_dims"),
        show: render("show"),
        frag_add: render("frag_add"),
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

/// One trial: zeroes a fresh `SIDE`×`SIDE` `r32uint` texture, adds 1 to every texel with atomics
/// from `writer`, runs `between`, reads it in a render pass and returns how many texels did not
/// read 1.
fn trial(gpu: &GpuContext, pipes: &Pipelines, writer: Writer, between: Between) -> u32 {
    let device = &gpu.device;
    let extent = wgpu::Extent3d {
        width: SIDE,
        height: SIDE,
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
        &vec![0; (4 * SIDE * SIDE) as usize],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * SIDE),
            rows_per_image: None,
        },
        extent,
    );
    let view = counter.create_view(&Default::default());
    let scratch = texture(wgpu::TextureUsages::RENDER_ATTACHMENT);
    let target = texture(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(4 * SIDE * SIDE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let touch = match between {
        Between::Nothing => None,
        Between::TouchOne | Between::TouchOneSamePass => Some((&pipes.touch_one, 1)),
        Between::TouchAll => Some((&pipes.touch_all, SIDE / 8)),
        Between::TouchDims => Some((&pipes.touch_dims, 1)),
    };
    let touch_group =
        touch.map(|(pipeline, _)| group(device, &pipeline.get_bind_group_layout(0), &view));
    let mut encoder = device.create_command_encoder(&Default::default());
    match writer {
        Writer::Compute => {
            let add_group = group(device, &pipes.add.get_bind_group_layout(0), &view);
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipes.add);
            pass.set_bind_group(0, &add_group, &[]);
            pass.dispatch_workgroups(SIDE / 8, SIDE / 8, 1);
            if between == Between::TouchOneSamePass {
                pass.set_pipeline(&pipes.touch_one);
                pass.set_bind_group(0, touch_group.as_ref().unwrap(), &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
        }
        Writer::Fragment => {
            let add_group = group(device, &pipes.frag_add.get_bind_group_layout(0), &view);
            draw(&mut encoder, &scratch, &pipes.frag_add, &add_group);
        }
    }
    if let (Some((pipeline, groups)), Some(touch_group)) = (touch, &touch_group)
        && between != Between::TouchOneSamePass
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, touch_group, &[]);
        pass.dispatch_workgroups(groups, groups, 1);
    }
    let show_group = group(device, &pipes.show.get_bind_group_layout(0), &view);
    draw(&mut encoder, &target, &pipes.show, &show_group);
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * SIDE),
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
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|texel| u32::from_le_bytes(**texel) != 1)
        .count() as u32
}

/// TEMPORARY DIAGNOSTIC: which follow-up pass makes atomics visible to a later render pass.
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
    const RUNS: usize = 50;
    const ROUNDS: usize = 4;
    let conditions = [
        (Writer::Compute, Between::Nothing),
        (Writer::Compute, Between::TouchOne),
        (Writer::Compute, Between::TouchAll),
        (Writer::Compute, Between::TouchDims),
        (Writer::Compute, Between::TouchOneSamePass),
        (Writer::Fragment, Between::Nothing),
        (Writer::Fragment, Between::TouchOne),
        (Writer::Fragment, Between::TouchAll),
    ];
    // (trials with any wrong texel, wrong texels)
    let mut totals: BTreeMap<(Writer, Between), (u32, u64)> = BTreeMap::new();
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
            for &(writer, between) in &conditions {
                let seen: Vec<(u32, u64)> = std::thread::scope(|inner| {
                    let workers: Vec<_> = devices
                        .iter()
                        .map(|(gpu, pipes)| {
                            inner.spawn(move || {
                                let mut seen = (0, 0);
                                for _ in 0..RUNS {
                                    let wrong = trial(gpu, pipes, writer, between);
                                    seen.0 += u32::from(wrong != 0);
                                    seen.1 += u64::from(wrong);
                                }
                                seen
                            })
                        })
                        .collect();
                    workers.into_iter().map(|w| w.join().unwrap()).collect()
                });
                let total = totals.entry((writer, between)).or_default();
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
    for ((writer, between), (trials, texels)) in &totals {
        report += &format!(
            "touch {writer:?} {between:?}: {trials} / {} trials wrong, {texels} texels wrong\n",
            THREADS * RUNS * ROUNDS
        );
    }
    panic!("DIAG\n{report}");
}
