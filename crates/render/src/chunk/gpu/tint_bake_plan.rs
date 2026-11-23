//! The tint bake kernel's constants, layouts, admission and dispatch. Plain `wgpu`, so the GPU
//! tests drive the kernel exactly as the renderer does.

use std::ops::Range;

/// Invocations per bake workgroup, substituted into the kernel as `TINT_BAKE_WORKGROUP_SIZE`.
pub(crate) const WORKGROUP_SIZE: u32 = 64;
/// Bytes of one cache entry: gamma RGB bits and the valid flag.
pub(crate) const ENTRY_BYTES: u64 = 16;
/// Storage bindings of group 0, at the chunk layout's numbers: quads, origins, materials, biome
/// records and biome tints. The biome query tables uniform follows.
pub(crate) const SOURCE_STORAGE_BINDINGS: [u32; 5] = [1, 2, 3, 7, 8];
/// Storage buffers in group 1: the jobs and the cache.
const OUTPUT_STORAGE_BINDINGS: u32 = 2;

/// One bake job: first packed quad, quad count, origin slot and padding.
pub(crate) type Job = [u32; 4];

/// The bake kernel with its generated constants resolved.
pub(crate) fn shader_source(source: &str) -> String {
    crate::material_shader::source(source)
        .replace("TINT_BAKE_WORKGROUP_SIZE", &format!("{WORKGROUP_SIZE}u"))
}

fn buffer(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Group 0: the packed-chunk tables the kernel reads.
pub(crate) fn source_layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
    SOURCE_STORAGE_BINDINGS
        .map(|binding| {
            buffer(
                binding,
                wgpu::BufferBindingType::Storage { read_only: true },
            )
        })
        .into_iter()
        .chain([buffer(
            crate::material_shader::BIOME_QUERY_TABLES_BINDING,
            wgpu::BufferBindingType::Uniform,
        )])
        .collect()
}

/// Group 1: the jobs at binding 0, then the cache the kernel writes at binding 1.
pub(crate) fn output_layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
    (0..OUTPUT_STORAGE_BINDINGS)
        .map(|binding| {
            buffer(
                binding,
                wgpu::BufferBindingType::Storage {
                    read_only: binding == 0,
                },
            )
        })
        .collect()
}

/// Whether a device can create and run the kernel: compute shaders, a workgroup of
/// [`WORKGROUP_SIZE`] x 1 x 1 lanes, both groups' storage buffers, the uniform and one entry.
pub(crate) fn kernel_supported(downlevel: wgpu::DownlevelFlags, limits: &wgpu::Limits) -> bool {
    downlevel.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        && limits.max_bind_groups >= 2
        && limits.max_compute_workgroup_size_x >= WORKGROUP_SIZE
        && limits.max_compute_workgroup_size_y >= 1
        && limits.max_compute_workgroup_size_z >= 1
        && limits.max_compute_invocations_per_workgroup >= WORKGROUP_SIZE
        && limits.max_compute_workgroups_per_dimension >= 1
        && limits.max_storage_buffers_per_shader_stage
            >= SOURCE_STORAGE_BINDINGS.len() as u32 + OUTPUT_STORAGE_BINDINGS
        && limits.max_uniform_buffers_per_shader_stage >= 1
        && limits.max_storage_buffer_binding_size >= ENTRY_BYTES
}

/// Entries for `quad_capacity` quads within a buffer and binding the device allows.
pub(crate) fn entry_count(quad_capacity: usize, limits: &wgpu::Limits) -> u64 {
    let bytes = limits
        .max_buffer_size
        .min(limits.max_storage_buffer_binding_size);
    (quad_capacity as u64).min(bytes / ENTRY_BYTES).max(1)
}

/// The job baking `quads` of origin slot `metadata` that fit `entries`, if any do.
pub(crate) fn job(quads: Range<u32>, metadata: u32, entries: u64) -> Option<Job> {
    let end = u64::from(quads.end).min(entries) as u32;
    let start = quads.start.min(end);
    (start < end).then_some([start, end - start, metadata, 0])
}

/// Pieces of `job` that each span at most `limit` workgroups along `x`.
pub(crate) fn split(job: Job, limit: u32) -> impl Iterator<Item = Job> {
    let [start, count, metadata, _] = job;
    let span = limit.max(1).saturating_mul(WORKGROUP_SIZE);
    (0..count.div_ceil(span)).map(move |piece| {
        let offset = piece * span;
        [start + offset, (count - offset).min(span), metadata, 0]
    })
}

/// Jobs one dispatch reaches along `y` and `z` within `limit` workgroups per axis.
pub(crate) fn reach(limit: u32) -> usize {
    (limit.max(1) as usize).saturating_mul(limit.max(1) as usize)
}

/// Workgroups covering planned `jobs`: `x` runs over a job's quads and `y + z * rows` selects
/// the job; every axis stays within `limit`.
pub(crate) fn dispatch_size(jobs: &[Job], limit: u32) -> [u32; 3] {
    let widest = jobs.iter().map(|job| job[1]).max().unwrap_or(0);
    let count = jobs.len() as u32;
    let rows = count.min(limit.max(1));
    [
        widest.div_ceil(WORKGROUP_SIZE),
        rows,
        count.div_ceil(rows.max(1)),
    ]
}

/// Bakes planned `jobs` on `pass`, whose pipeline and groups are already set.
pub(crate) fn dispatch(pass: &mut wgpu::ComputePass<'_>, jobs: &[Job], limit: u32) {
    let [x, y, z] = dispatch_size(jobs, limit);
    if x > 0 && y > 0 && z > 0 {
        pass.dispatch_workgroups(x, y, z);
    }
}

/// Zeroes the entries of `jobs`, so the vertex stage computes their tints until they bake.
pub(crate) fn clear(encoder: &mut wgpu::CommandEncoder, cache: &wgpu::Buffer, jobs: &[Job]) {
    let entries = cache.size() / ENTRY_BYTES;
    for &[start, count, ..] in jobs {
        let end = (u64::from(start) + u64::from(count)).min(entries);
        let start = u64::from(start).min(end);
        if start < end {
            encoder.clear_buffer(
                cache,
                start * ENTRY_BYTES,
                Some((end - start) * ENTRY_BYTES),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_fit_both_the_buffer_and_binding_limits() {
        let limits = |buffer, binding| wgpu::Limits {
            max_buffer_size: buffer,
            max_storage_buffer_binding_size: binding,
            ..wgpu::Limits::default()
        };
        assert_eq!(entry_count(100, &limits(u64::MAX, u64::MAX)), 100);
        assert_eq!(
            entry_count(100, &limits(64 * ENTRY_BYTES + 15, u64::MAX)),
            64
        );
        assert_eq!(entry_count(100, &limits(u64::MAX, 32 * ENTRY_BYTES)), 32);
        assert_eq!(entry_count(0, &limits(u64::MAX, u64::MAX)), 1);
    }

    #[test]
    fn kernel_admission_requires_compute_and_every_limit() {
        let compute = wgpu::DownlevelFlags::COMPUTE_SHADERS;
        let limits = wgpu::Limits::default();
        assert!(kernel_supported(compute, &limits));
        assert!(!kernel_supported(wgpu::DownlevelFlags::empty(), &limits));
        for narrowed in [
            wgpu::Limits {
                max_compute_workgroup_size_x: WORKGROUP_SIZE - 1,
                ..limits.clone()
            },
            wgpu::Limits {
                max_compute_invocations_per_workgroup: WORKGROUP_SIZE - 1,
                ..limits.clone()
            },
            wgpu::Limits {
                max_storage_buffers_per_shader_stage: 6,
                ..limits.clone()
            },
            wgpu::Limits {
                max_compute_workgroups_per_dimension: 0,
                ..limits.clone()
            },
            wgpu::Limits {
                max_compute_workgroup_size_y: 0,
                ..limits.clone()
            },
            wgpu::Limits {
                max_compute_workgroup_size_z: 0,
                ..limits.clone()
            },
        ] {
            assert!(!kernel_supported(compute, &narrowed));
        }
    }

    #[test]
    fn pieces_and_dispatches_stay_within_each_axis_limit() {
        assert_eq!(
            split([0, 130, 7, 0], 65_535).collect::<Vec<_>>(),
            [[0, 130, 7, 0]]
        );
        // Two workgroups per axis: 128-quad pieces, four jobs per dispatch.
        assert_eq!(
            split([0, 130, 7, 0], 2).collect::<Vec<_>>(),
            [[0, 128, 7, 0], [128, 2, 7, 0]]
        );
        assert_eq!(reach(2), 4);
        let jobs = [
            [0, 128, 0, 0],
            [128, 2, 0, 0],
            [200, 1, 1, 0],
            [300, 64, 2, 0],
        ];
        assert!(dispatch_size(&jobs, 2).iter().all(|&axis| axis <= 2));
        assert_eq!(dispatch_size(&jobs[..3], 65_535), [2, 3, 1]);
    }

    #[test]
    fn jobs_clip_to_the_cache() {
        assert_eq!(job(10..20, 3, 15), Some([10, 5, 3, 0]));
        assert_eq!(job(10..20, 3, 10), None);
        assert_eq!(job(5..5, 3, 100), None);
    }
}
