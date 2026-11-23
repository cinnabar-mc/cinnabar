// Bakes the per-quad tint cache that cube vertex programs read (`chunk/gpu/tint_cache.rs`).
import render::chunk::tint_cache_entry;

// The job count in `x` of the first element, then per job: first packed quad, quad count,
// origin slot and padding. The buffer persists, so entries past the count are stale.
@group(1) @binding(0) var<storage, read> bake_jobs: array<vec4<u32>>;
@group(1) @binding(1) var<storage, read_write> baked_tints: array<vec4<u32>>;

// Workgroup `y + z * rows` selects a job and `x` a run of its quads.
@compute @workgroup_size(TINT_BAKE_WORKGROUP_SIZE)
fn bake(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let job_index = group.y + group.z * groups.y;
    if (job_index >= bake_jobs[0].x || job_index + 1u >= arrayLength(&bake_jobs)) { return; }
    let job = bake_jobs[job_index + 1u];
    let local = group.x * TINT_BAKE_WORKGROUP_SIZE + lane;
    if (local >= job.y) { return; }
    let quad = job.x + local;
    if (quad >= arrayLength(&baked_tints)) { return; }
    baked_tints[quad] = tint_cache_entry(quad, job.z);
}
