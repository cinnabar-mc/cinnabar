//! Fused dot accumulation through the translator and the active GPU backend.

use super::*;
use wgpu::util::DeviceExt;

/// Supplies dynamic binary32 operands and reads the translated dot result as raw bits.
const COMPUTE: &str = "#version 450
layout(local_size_x=64) in;
layout(std430, binding=0) buffer Samples { uvec4 samples[]; };
void main() {
    uint i = gl_GlobalInvocationID.x;
    if (i >= samples.length()) return;
    vec3 v = uintBitsToFloat(samples[i].xyz);
    samples[i].w = floatBitsToUint(dot(vec2(v.z, v.x), vec2(1.0, v.y)));
}
";

/// Builds edge products and deterministic random samples across all binary32 bit patterns.
fn samples() -> Vec<[u32; 4]> {
    let edges = [
        0.0_f32.to_bits(),
        (-0.0_f32).to_bits(),
        1,
        0x007f_ffff,
        0x0080_0000,
        0x3f00_0000,
        0x3f80_0000,
        0x3f80_0001,
        0x4000_0000,
        0x7f7f_ffff,
        f32::INFINITY.to_bits(),
        f32::NEG_INFINITY.to_bits(),
        f32::NAN.to_bits(),
    ];
    let mut samples = Vec::new();
    for a in edges {
        for b in edges {
            for c in edges {
                samples.push([a, b, c, 0]);
                samples.push([a ^ (1 << 31), b, c ^ (1 << 31), 0]);
            }
        }
    }
    let mut state = 0x8b38_0a41_u32;
    for _ in 0..16_384 {
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let [a, b, c] = [next(), next(), next()];
        samples.push([a, b, c, 0]);
        // Near cancellation exercises every retained product bit.
        let product = f32::from_bits(a) * f32::from_bits(b);
        if product.is_normal() {
            samples.push([a, b, (-product).to_bits(), 0]);
        }
    }
    samples
}

#[test]
fn software_fused_dot_matches_binary32_mul_add() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for software fused dot");
        return;
    };
    for [a, b, c, got] in dot_results(&gpu, true) {
        let [a, b, c, got] = [a, b, c, got].map(f32::from_bits);
        let expected = a.mul_add(b, c);
        assert!(
            (expected.is_nan() && got.is_nan()) || expected.to_bits() == got.to_bits(),
            "{a:?} * {b:?} + {c:?}: {got:?} versus {expected:?}"
        );
    }
}

/// A device whose `fma` the load's probe finds rounding once gives the reference rounding
/// through the native helpers for every normal operand and result.
#[test]
fn native_dot_matches_binary32_mul_add_where_the_probe_finds_single_rounding() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for native fused dot");
        return;
    };
    if !super::super::fused_fma::rounds_once(&gpu) {
        eprintln!("missing fixture: an adapter whose fma rounds once");
        return;
    }
    for [a, b, c, got] in dot_results(&gpu, false) {
        let [a, b, c, got] = [a, b, c, got].map(f32::from_bits);
        let expected = a.mul_add(b, c);
        if [a, b, c, a * b, expected].iter().all(|v| v.is_normal()) {
            assert_eq!(
                expected.to_bits(),
                got.to_bits(),
                "{a:?} * {b:?} + {c:?}: {got:?} versus {expected:?}"
            );
        }
    }
}

/// Runs [`COMPUTE`]'s translated dot over [`samples`] and returns each sample with its result.
fn dot_results(gpu: &GpuContext, software_fused_dot: bool) -> Vec<[u32; 4]> {
    let pack = pack(&[("composite.csh", COMPUTE)]);
    let translation = pack.translate(&shaderpack::TranslateConfig {
        software_fused_dot,
        ..Default::default()
    });
    let program = &translation.programs[0];
    assert!(
        program.is_ok(),
        "{:?}",
        program.all_diagnostics().collect::<Vec<_>>()
    );
    let stage = program.stages[0].result.as_ref().unwrap();
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("software fused dot"),
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(stage.module.clone())),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let samples = samples();
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&samples),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(shaderpack::STORAGE_BUFFER_GROUP),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        // The translated storage group follows three empty interface groups.
        for index in 0..shaderpack::STORAGE_BUFFER_GROUP {
            let empty = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(index),
                entries: &[],
            });
            pass.set_bind_group(index, &empty, &[]);
        }
        pass.set_bind_group(shaderpack::STORAGE_BUFFER_GROUP, &group, &[]);
        pass.dispatch_workgroups((samples.len() as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&buffer, 0, &readback, 0, buffer.size());
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = readback.slice(..).get_mapped_range().unwrap();
    bytemuck::cast_slice::<u8, [u32; 4]>(&bytes).to_vec()
}
