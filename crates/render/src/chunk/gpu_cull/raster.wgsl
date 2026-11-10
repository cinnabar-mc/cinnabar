// Shared record and view declarations are supplied by raster::shader_source.
const SEAM_MAX_STEP: f32 = RASTER_SEAM_MAX_STEP;
@group(0) @binding(0) var<uniform> view: CullView;
@group(0) @binding(1) var<storage, read> records: array<CullRecord>;
@group(0) @binding(3) var<storage, read_write> history: array<atomic<u32>>;

// Decodes one local bound without introducing a second record layout.
fn local_bound(word: u32) -> vec3<f32> {
    return vec3<f32>(vec3<i32>(i32(word & 255u), i32((word >> 8u) & 255u),
        i32((word >> 16u) & 255u)) - vec3(BOUNDS_BIAS));
}

// Projects a camera-relative corner of the supplied conservative bounds.
fn box_corner(record: CullRecord, low: vec3<f32>, high: vec3<f32>, corner: u32) -> vec4<f32> {
    let pick = vec3((corner & 1u) != 0u, (corner & 2u) != 0u, (corner & 4u) != 0u);
    let origin = vec3(record.origin_x, record.origin_y, record.origin_z);
    return view.clip_from_rel * vec4(vec3<f32>(origin - view.camera.xyz) + select(low, high, pick), 1.0);
}

struct ProjectedBox {
    low: vec2<f32>,
    high: vec2<f32>,
    nearest: f32,
    unconditional: bool,
}

// Encloses original geometry and every clamped cube-seal step before projecting its rectangle.
fn project_box(record: CullRecord) -> ProjectedBox {
    var result = ProjectedBox(vec2(1.0e30), vec2(-1.0e30), 0.0, false);
    var low = local_bound(record.bounds_min) - vec3(HIZ_PADDING);
    var high = local_bound(record.bounds_max) + vec3(HIZ_PADDING);
    var max_w = 0.0;
    for (var corner = 0u; corner < 8u; corner++) {
        let clip = box_corner(record, low, high, corner);
        if (!(clip.w > 1.0e-4) || !(clip.z < clip.w) || !all(abs(clip) < vec4(1.0e30))) {
            result.unconditional = true;
            return result;
        }
        max_w = max(max_w, abs(clip.w));
    }
    // Each in-plane axis moves by at most this shader-owned world-space step.
    let seal = SEAM_MAX_STEP * max_w;
    low -= vec3(seal);
    high += vec3(seal);
    for (var corner = 0u; corner < 8u; corner++) {
        let clip = box_corner(record, low, high, corner);
        if (!(clip.w > 1.0e-4) || !(clip.z < clip.w) || !all(abs(clip) < vec4(1.0e30))) {
            result.unconditional = true;
            return result;
        }
        let ndc = clip.xyz / clip.w;
        result.low = min(result.low, ndc.xy);
        result.high = max(result.high, ndc.xy);
        result.nearest = max(result.nearest, ndc.z);
    }
    let pixels = vec2(2.0 * PIXEL_PADDING) / view.viewport.zw;
    result.low -= pixels;
    result.high += pixels;
    let size = max(min(result.high, vec2(1.0)) - max(result.low, vec2(-1.0)), vec2(0.0));
    result.unconditional = size.x * size.y > 1.0;
    return result;
}

// The old classification can select early depth only; current coverage is reset every test.
@compute @workgroup_size(WORKGROUP)
fn raster_prepare(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x < view.params.x) {
        let old = select(0u, HISTORY_VISIBLE, (atomicLoad(&history[gid.x]) & HISTORY_RASTER_VISIBLE) != 0u);
        atomicStore(&history[gid.x], old | select(0u, HISTORY_RASTER_VISIBLE, project_box(records[gid.x]).unconditional));
    }
}

struct BoxVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) slot: u32,
}

// Rasterises the padded projected rectangle at the nearest conservative box depth.
@vertex
fn raster_box(@builtin(vertex_index) vertex: u32, @builtin(instance_index) slot: u32) -> BoxVertex {
    let record = records[slot];
    let bounds = project_box(record);
    var out: BoxVertex;
    out.slot = slot;
    out.position = vec4(2.0, 2.0, 0.0, 1.0);
    if (record.live == 0u || bounds.unconditional) {
        return out;
    }
    let triangles = array<u32, RASTER_VERTEX_COUNT>(0u, 1u, 3u, 0u, 3u, 2u);
    let corner = triangles[vertex];
    let pick = vec2((corner & 1u) != 0u, (corner & 2u) != 0u);
    out.position = vec4(select(bounds.low, bounds.high, pick), bounds.nearest, 1.0);
    return out;
}

// Only passing physical depth samples may publish current visibility.
@fragment @early_depth_test(force)
fn raster_seen(in: BoxVertex) {
    atomicOr(&history[in.slot], HISTORY_RASTER_VISIBLE);
}
