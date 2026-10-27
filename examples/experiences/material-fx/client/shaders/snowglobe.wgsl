// Snow globe: a glass block holding a snowy mound, a pixel pine with a gold star and a tiny
// cabin, all made of block texels and traced per pixel so they sit at their true depth from
// every angle. Snow falls through the globe on three depth layers, each flake one texel,
// drifting sideways as it falls, and settles on whatever stands in front.
// param 0: snowfall speed in texels per second; param 1: glass tint strength.

struct Hit {
    t: f32,
    normal: vec3<f32>,
    colour: vec3<f32>,
}

// `best`, or the box [lo, hi] (in texels) of `colour` when the ray meets it sooner.
fn solid(best: Hit, o: vec3<f32>, d: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>, colour: vec3<f32>) -> Hit {
    let inverse = 1.0 / select(d, vec3<f32>(1e-6), abs(d) < vec3<f32>(1e-6));
    let a = (lo - o) * inverse;
    let b = (hi - o) * inverse;
    let near3 = min(a, b);
    let near = max(max(near3.x, near3.y), near3.z);
    let far = min(min(max(a.x, b.x), max(a.y, b.y)), max(a.z, b.z));
    if far < max(near, 0.0) || near < 0.0 || near >= best.t {
        return best;
    }
    var normal = vec3<f32>(0.0, 0.0, -sign(d.z));
    if near == near3.x {
        normal = vec3<f32>(-sign(d.x), 0.0, 0.0);
    } else if near == near3.y {
        normal = vec3<f32>(0.0, -sign(d.y), 0.0);
    }
    return Hit(near, normal, colour);
}

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let local = local_of(input);
    let o = local * 16.0;
    let d = -input.view;
    var hit = Hit(1e9, vec3<f32>(0.0), vec3<f32>(0.0));
    hit = solid(hit, o, d, vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(15.0, 3.0, 15.0), vec3<f32>(0.93, 0.96, 1.0));
    hit = solid(hit, o, d, vec3<f32>(4.0, 3.0, 4.0), vec3<f32>(12.0, 4.0, 12.0), vec3<f32>(0.97, 0.98, 1.0));
    hit = solid(hit, o, d, vec3<f32>(7.0, 4.0, 7.0), vec3<f32>(9.0, 6.0, 9.0), vec3<f32>(0.4, 0.26, 0.14));
    hit = solid(hit, o, d, vec3<f32>(5.0, 6.0, 5.0), vec3<f32>(11.0, 8.0, 11.0), vec3<f32>(0.1, 0.38, 0.18));
    hit = solid(hit, o, d, vec3<f32>(6.0, 8.0, 6.0), vec3<f32>(10.0, 10.0, 10.0), vec3<f32>(0.12, 0.45, 0.2));
    hit = solid(hit, o, d, vec3<f32>(7.0, 10.0, 7.0), vec3<f32>(9.0, 12.0, 9.0), vec3<f32>(0.15, 0.52, 0.24));
    hit = solid(hit, o, d, vec3<f32>(7.5, 12.0, 7.5), vec3<f32>(8.5, 13.0, 8.5), vec3<f32>(1.0, 0.85, 0.2));
    hit = solid(hit, o, d, vec3<f32>(11.0, 3.0, 2.0), vec3<f32>(14.0, 6.0, 5.0), vec3<f32>(0.55, 0.32, 0.18));
    hit = solid(hit, o, d, vec3<f32>(11.0, 6.0, 2.0), vec3<f32>(14.0, 7.0, 5.0), vec3<f32>(0.85, 0.15, 0.12));
    // Snow on three depth layers across the face, nearest first.
    let frame_w = -input.normal;
    let speed = max(param(input, 0u), 0.5);
    var flake = vec4<f32>(0.0);
    let depths = vec3<f32>(3.0, 8.0, 13.0);
    let d_w = max(dot(d, frame_w), 0.05);
    let layer0 = o + d * (depths.x / d_w);
    let layer1 = o + d * (depths.y / d_w);
    let layer2 = o + d * (depths.z / d_w);
    let fall = time * speed;
    let cell0 = floor(vec3<f32>(layer0.x + sin(time + layer0.y * 0.3) * 1.5, layer0.y + fall, layer0.z));
    let cell1 = floor(vec3<f32>(layer1.x + sin(time * 0.8 + layer1.y * 0.3) * 1.5, layer1.y + fall * 0.8, layer1.z));
    let cell2 = floor(vec3<f32>(layer2.x + sin(time * 0.6 + layer2.y * 0.3) * 1.5, layer2.y + fall * 0.6, layer2.z));
    let inside0 = all(layer0 > vec3<f32>(0.0)) && all(layer0 < vec3<f32>(16.0));
    let inside1 = all(layer1 > vec3<f32>(0.0)) && all(layer1 < vec3<f32>(16.0));
    let inside2 = all(layer2 > vec3<f32>(0.0)) && all(layer2 < vec3<f32>(16.0));
    let t0 = depths.x / d_w;
    let t1 = depths.y / d_w;
    let t2 = depths.z / d_w;
    if inside2 && t2 < hit.t && hash3(cell2) > 0.95 {
        flake = vec4<f32>(0.85, 0.9, 1.0, t2);
    }
    if inside1 && t1 < hit.t && hash3(cell1 + 3.0) > 0.95 {
        flake = vec4<f32>(0.92, 0.95, 1.0, t1);
    }
    if inside0 && t0 < hit.t && hash3(cell0 + 7.0) > 0.95 {
        flake = vec4<f32>(1.0, 1.0, 1.0, t0);
    }
    let uv = face_uv(local, input.normal);
    if min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y)) < 1.0 / 16.0 {
        return Surface(vec3<f32>(0.35, 0.2, 0.1), 1.0, vec3<f32>(0.0));
    }
    if flake.a > 0.0 {
        return Surface(flake.rgb, 1.0, flake.rgb * 0.2);
    }
    if hit.t < 1e8 {
        let lit = floor((0.6 + 0.4 * dot(hit.normal, normalize(vec3<f32>(0.4, 1.0, 0.3)))) * 4.0 + 0.5) / 4.0;
        let star = select(vec3<f32>(0.0), hit.colour * 1.5, hit.colour.b < 0.3 && hit.colour.r > 0.9);
        let window = select(vec3<f32>(0.0), vec3<f32>(1.0, 0.75, 0.3), hit.colour.r > 0.5 && hit.colour.r < 0.6 && abs(hit.normal.z) > 0.5);
        return Surface(hit.colour * lit, 1.0, star + window * 0.8);
    }
    return Surface(vec3<f32>(0.75, 0.88, 1.0), clamp(param(input, 1u), 0.0, 1.0), vec3<f32>(0.0));
}
