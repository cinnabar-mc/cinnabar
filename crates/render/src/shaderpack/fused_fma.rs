//! Whether the device's own `fma` rounds once, as the reference dot accumulation needs. HLSL
//! and SPIR-V leave fusing to the driver, so it is measured on the device rather than assumed.

use super::pipelines::{GpuContext, RejectionScope};
use std::{sync::mpsc, time::Duration};

/// Longest wait for each of the probe's GPU steps; a slower device keeps the software path.
const PROBE_WAIT: Duration = Duration::from_secs(5);

/// Writes `fma(x, y, z)` of each sample into its `w` bits.
const PROBE: &str = "
@group(0) @binding(0) var<storage, read_write> samples: array<vec4<u32>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= arrayLength(&samples)) {
        return;
    }
    let s = samples[id.x];
    samples[id.x].w = bitcast<u32>(fma(bitcast<f32>(s.x), bitcast<f32>(s.y), bitcast<f32>(s.z)));
}
";

/// Operands whose single rounding differs from rounding `a * b` first: `c` cancels the
/// rounded product, so a fused result is the product's nonzero rounding error and an unfused
/// one is zero. Every operand and result is a normal number.
fn samples() -> Vec<[u32; 4]> {
    let mut state = 0x2545_f491_u32;
    let mut mantissa = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        0x3f80_0000 | state >> 9
    };
    let mut samples = Vec::new();
    while samples.len() < 64 {
        let (a, b) = (f32::from_bits(mantissa()), f32::from_bits(mantissa()));
        if a.mul_add(b, -(a * b)) != 0.0 {
            samples.push([a.to_bits(), b.to_bits(), (-(a * b)).to_bits(), 0]);
        }
    }
    samples
}

/// Whether `fma` on `gpu` matched [`f32::mul_add`] on every probe operand. Any failure to run
/// the probe, including a device that does not answer within [`PROBE_WAIT`], answers false.
pub(super) fn rounds_once(gpu: &GpuContext) -> bool {
    probe(gpu).is_some_and(|results| {
        results.iter().all(|&[a, b, c, got]| {
            f32::from_bits(a)
                .mul_add(f32::from_bits(b), f32::from_bits(c))
                .to_bits()
                == got
        })
    })
}

/// Each probe operand with the device's `fma` result in its last word, or `None` when the
/// device rejects the probe, cannot allocate it or does not answer in time.
fn probe(gpu: &GpuContext) -> Option<Vec<[u32; 4]>> {
    let samples = samples();
    let scope = RejectionScope::push(&gpu.device);
    let bytes: &[u8] = bytemuck::cast_slice(&samples);
    let size = bytes.len() as u64;
    let create = |label, usage| {
        gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let render_device = bevy::render::renderer::RenderDevice::from(gpu.device.clone());
    // Out-of-memory errors fall outside the rejection scope, so the whole submission sits in
    // an allocation check; a failed probe never reads its results.
    let (readback, submission) =
        crate::gpu_allocation::checked(&render_device, "shader pack fma probe", 2 * size, || {
            let module = gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("shader pack fma probe"),
                    source: wgpu::ShaderSource::Wgsl(PROBE.into()),
                });
            let pipeline = gpu
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("shader pack fma probe"),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
            let buffer = create(
                "shader pack fma probe",
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
            );
            let readback = create(
                "shader pack fma probe readback",
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            );
            gpu.queue.write_buffer(&buffer, 0, bytes);
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shader pack fma probe"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups((samples.len() as u32).div_ceil(64), 1, 1);
            }
            encoder.copy_buffer_to_buffer(&buffer, 0, &readback, 0, size);
            let submission = gpu.queue.submit([encoder.finish()]);
            (readback, submission)
        })?;
    if scope.pop().is_some() {
        return None;
    }
    let (send, receive) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
    let waited = gpu.device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(PROBE_WAIT),
    });
    // Another thread polling the device may run the callback, so it gets its own bound.
    if waited.is_err() || !matches!(receive.recv_timeout(PROBE_WAIT), Ok(Ok(()))) {
        return None;
    }
    let results =
        bytemuck::cast_slice::<u8, [u32; 4]>(&readback.slice(..).get_mapped_range().ok()?).to_vec();
    readback.unmap();
    Some(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_operands_tell_fused_from_unfused_rounding() {
        for [a, b, c, _] in samples() {
            let [a, b, c] = [a, b, c].map(f32::from_bits);
            let fused = a.mul_add(b, c);
            assert!(fused.is_normal() && a * b + c == 0.0, "{a:?} {b:?} {c:?}");
        }
    }

    /// The probe reads back one of the two roundings, so its verdict reflects the device.
    #[test]
    fn the_probe_reads_back_single_or_double_rounding() {
        let Some(gpu) = super::super::tests::gpu() else {
            eprintln!("missing fixture: native GPU adapter for the fma probe");
            return;
        };
        let results = probe(&gpu).expect("a native adapter answers the probe");
        let fused = results.iter().map(|&[a, b, c, got]| {
            let [a, b, c] = [a, b, c].map(f32::from_bits);
            if got == a.mul_add(b, c).to_bits() {
                true
            } else {
                assert_eq!(f32::from_bits(got), a * b + c, "{a:?} * {b:?} + {c:?}");
                false
            }
        });
        let fused: Vec<bool> = fused.collect();
        assert!(
            fused.iter().all(|&f| f == fused[0]),
            "one rounding throughout"
        );
        assert_eq!(rounds_once(&gpu), fused[0]);
    }

    /// A context on the no-op backend, which runs no shader work.
    fn noop_gpu() -> GpuContext {
        let (device, queue) = wgpu::Device::noop(&Default::default());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::NOOP,
            backend_options: wgpu::BackendOptions {
                noop: wgpu::NoopBackendOptions {
                    enable: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
        GpuContext {
            device,
            queue,
            adapter,
        }
    }

    #[test]
    fn a_device_that_cannot_answer_keeps_the_software_path() {
        assert!(!rounds_once(&noop_gpu()));
    }

    #[test]
    fn a_probe_without_memory_keeps_the_software_path() {
        let _failing = crate::gpu_allocation::inject(|label, _| label == "shader pack fma probe");
        let gpu = noop_gpu();
        assert!(probe(&gpu).is_none());
        assert!(!rounds_once(&gpu));
    }
}
