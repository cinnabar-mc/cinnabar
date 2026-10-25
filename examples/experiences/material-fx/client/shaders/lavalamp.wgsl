// Lava lamp: glowing pixel blobs rise and sink and merge in violet liquid inside a glass block.
// The view ray is sampled at twelve depths through the block; at each, the metaball field is
// read at the centre of the texel voxel the point falls in, so the blobs are built from block
// texels. The first voxel inside a blob wins.
// params 0-2: blob colour; param 3: speed.

fn field(v: vec3<f32>, time: f32) -> f32 {
    let a = vec3<f32>(8.0 + 2.0 * sin(time * 0.7), 8.0 + 5.5 * sin(time * 0.5), 8.0 + 2.0 * cos(time * 0.6));
    let b = vec3<f32>(8.0 + 2.5 * cos(time * 0.45), 8.0 + 5.5 * sin(time * 0.38 + 2.1), 8.0 + 2.0 * sin(time * 0.8));
    let c = vec3<f32>(8.0 + 2.0 * sin(time * 0.33 + 1.0), 8.0 + 5.0 * cos(time * 0.29), 8.0 + 2.5 * cos(time * 0.4 + 0.5));
    let pool = max(4.5 - v.y, 0.0) * 0.35;
    return 4.0 / dot(v - a, v - a) + 3.0 / dot(v - b, v - b) + 2.5 / dot(v - c, v - c) + pool;
}

fn sample_at(o: vec3<f32>, d: vec3<f32>, t: f32, time: f32, found: vec4<f32>) -> vec4<f32> {
    if found.a > 0.0 {
        return found;
    }
    let p = o + d * t;
    if any(p < vec3<f32>(1.0)) || any(p > vec3<f32>(15.0)) {
        return found;
    }
    let voxel = floor(p) + 0.5;
    let value = field(voxel, time);
    if value < 1.0 {
        return found;
    }
    return vec4<f32>(voxel, t);
}

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input) * max(param(input, 3u), 0.1);
    let local = local_of(input);
    let o = local * 16.0;
    let d = -input.view;
    let span = box_exit(o, d, vec3<f32>(0.0), vec3<f32>(16.0));
    let step = span / 12.0;
    var found = vec4<f32>(0.0);
    found = sample_at(o, d, step * 0.5, time, found);
    found = sample_at(o, d, step * 1.5, time, found);
    found = sample_at(o, d, step * 2.5, time, found);
    found = sample_at(o, d, step * 3.5, time, found);
    found = sample_at(o, d, step * 4.5, time, found);
    found = sample_at(o, d, step * 5.5, time, found);
    found = sample_at(o, d, step * 6.5, time, found);
    found = sample_at(o, d, step * 7.5, time, found);
    found = sample_at(o, d, step * 8.5, time, found);
    found = sample_at(o, d, step * 9.5, time, found);
    found = sample_at(o, d, step * 10.5, time, found);
    found = sample_at(o, d, step * 11.5, time, found);
    let uv = face_uv(local, input.normal);
    let edge = min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y)) < 1.0 / 16.0;
    let cap = local.y > 13.0 / 16.0 || local.y < 3.0 / 16.0;
    if edge || cap {
        return Surface(vec3<f32>(0.6, 0.62, 0.68) * select(0.7, 1.0, local.y > 0.5), 1.0, vec3<f32>(0.0));
    }
    let blob = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    if found.a > 0.0 {
        // Blobs glow hotter towards the lamp's base, in four flat steps by height.
        let shade_level = floor((1.0 - found.y / 16.0) * 4.0 + 1.0) / 4.0;
        return Surface(blob * shade_level, 1.0, blob * shade_level * 1.6);
    }
    let liquid = posterize(mix(vec3<f32>(0.25, 0.05, 0.4), vec3<f32>(0.45, 0.1, 0.55), local.y), 6.0);
    return Surface(liquid, 1.0, liquid * 0.5);
}
