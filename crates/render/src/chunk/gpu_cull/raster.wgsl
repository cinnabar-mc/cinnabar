// Shared record and view declarations are supplied by raster::shader_source.
@group(0) @binding(0) var<uniform> view: CullView;
@group(0) @binding(1) var<storage, read> records: array<CullRecord>;
@group(0) @binding(3) var<storage, read_write> history: array<atomic<u32>>;

// Transforms one padded local bound corner relative to the integer camera.
fn box_corner(record: CullRecord, corner: u32) -> vec4<f32> {
    let low = vec3<i32>(i32(record.bounds_min & 255u),
        i32((record.bounds_min >> 8u) & 255u), i32((record.bounds_min >> 16u) & 255u));
    let high = vec3<i32>(i32(record.bounds_max & 255u),
        i32((record.bounds_max >> 8u) & 255u), i32((record.bounds_max >> 16u) & 255u));
    let pick = vec3((corner & 1u) != 0u, (corner & 2u) != 0u, (corner & 4u) != 0u);
    let local = select(vec3<f32>(low - vec3(BOUNDS_BIAS)) - vec3(HIZ_PADDING),
        vec3<f32>(high - vec3(BOUNDS_BIAS)) + vec3(HIZ_PADDING), pick);
    let origin = vec3(record.origin_x, record.origin_y, record.origin_z);
    return view.clip_from_rel * vec4(vec3<f32>(origin - view.camera.xyz) + local, 1.0);
}

// Clipping the near faces, excessive raster work and invalid projection all keep the section.
fn conservative(record: CullRecord) -> bool {
    var lo = vec2(1.0e30);
    var hi = vec2(-1.0e30);
    for (var corner = 0u; corner < 8u; corner++) {
        let clip = box_corner(record, corner);
        if (!(clip.w > 1.0e-4) || !(clip.z < clip.w) ||
            !all(abs(clip) < vec4(1.0e30))) {
            return true;
        }
        lo = min(lo, clip.xy / clip.w);
        hi = max(hi, clip.xy / clip.w);
    }
    let size = max(min(hi, vec2(1.0)) - max(lo, vec2(-1.0)), vec2(0.0));
    return size.x * size.y > 1.0;
}

// Bit zero still belongs to the previous completed frame; bit one is reset every test.
@compute @workgroup_size(WORKGROUP)
fn raster_prepare(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x < view.params.x) {
        let old = select(0u, HISTORY_VISIBLE, (atomicLoad(&history[gid.x]) & HISTORY_RASTER_VISIBLE) != 0u);
        atomicStore(&history[gid.x], old | select(0u, HISTORY_RASTER_VISIBLE, conservative(records[gid.x])));
    }
}

struct BoxVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) slot: u32,
}

// Generates every face of one live, conservatively testable section box.
@vertex
fn raster_box(@builtin(vertex_index) vertex: u32, @builtin(instance_index) slot: u32) -> BoxVertex {
    let record = records[slot];
    var out: BoxVertex;
    out.slot = slot;
    out.position = vec4(2.0, 2.0, 0.0, 1.0);
    if (record.live == 0u || conservative(record)) {
        return out;
    }
    // Two triangles on each face. No face is culled, so winding cannot lose coverage.
    let face = vertex / 6u;
    let axis = face / 2u;
    let a = (axis + 1u) % 3u;
    let b = (axis + 2u) % 3u;
    let triangle = array<u32, 6>(0u, 1u, 3u, 0u, 3u, 2u);
    let uv = triangle[vertex % 6u];
    let corner = ((face & 1u) << axis) | ((uv & 1u) << a) | (((uv >> 1u) & 1u) << b);
    out.position = box_corner(record, corner);
    return out;
}

// Only samples passing the fixed-function depth test can publish visibility.
@fragment @early_depth_test(force)
fn raster_seen(in: BoxVertex) {
    atomicOr(&history[in.slot], HISTORY_RASTER_VISIBLE);
}
