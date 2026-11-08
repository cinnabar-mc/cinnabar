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

/// Uses each atomic's result, so the GPU has to wait for every atomic to return.
const RETURNING_ATOMIC: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    uint previous = imageAtomicAdd(counter, ivec2(0), 1u);
    if (previous == 0xFFFFFFFFu) {
        imageStore(counter, ivec2(0), uvec4(0u));
    }
}
";

const CLEARED: &str =
    "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT true false 1 1\n";
const KEPT: &str =
    "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT false false 1 1\n";

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
    const LOAD_THREADS: usize = 2;
    const RUNS: usize = 25;
    const ROUNDS: usize = 8;
    // (name, properties, compute, frames, expected red)
    let variants: [(&str, &str, &str, usize, u8); 5] = [
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
        (
            "N returning atomic, kept, 1 frame",
            KEPT,
            RETURNING_ATOMIC,
            1,
            16,
        ),
    ];
    let mut totals: BTreeMap<(usize, bool), BTreeMap<u8, u32>> = BTreeMap::new();
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let loaders: Vec<_> = (0..LOAD_THREADS)
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
