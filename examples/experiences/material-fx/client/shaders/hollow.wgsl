// Hollow blocks: each face opens onto a room far deeper than the block, found per pixel by
// intersecting the view ray with the room's walls (interior mapping), with every hit snapped to
// the 16-texel grid of its own depth. Mode 0 is an aquarium: water fading to deep blue, a sand
// floor with rippling light, swaying kelp, rising bubbles and pixel fish swimming at three
// depths. Mode 1 is a mineshaft: timber frames and rails running into the dark towards a
// flickering torch.
// param 0: mode; param 1: room depth in blocks.

struct Frame {
    u: vec3<f32>,
    v: vec3<f32>,
    w: vec3<f32>,
}

// The face's across, up and inward axes.
fn frame_of(normal: vec3<f32>) -> Frame {
    let n = abs(normal);
    if n.y > 0.5 {
        return Frame(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), -normal);
    }
    if n.x > 0.5 {
        return Frame(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 1.0, 0.0), -normal);
    }
    return Frame(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), -normal);
}

// Room coordinates (across, up, depth) where the ray from `origin` along `dir` meets the plane
// at `depth`, and whether it does so inside the room before `limit`.
fn plane_hit(origin: vec3<f32>, dir: vec3<f32>, depth: f32, limit: f32) -> vec4<f32> {
    let t = (depth - origin.z) / max(dir.z, 1e-4);
    let p = origin + dir * t;
    let inside = t > 0.0 && t < limit && all(p.xy >= vec2<f32>(0.0)) && all(p.xy <= vec2<f32>(1.0));
    return vec4<f32>(p, select(-1.0, t, inside));
}

// A pixel fish of `colour` centred at `centre` on a plane, facing `facing` (+1 or -1): the
// texel offset decides body, tail, stripe or eye; alpha 0 where there is no fish.
fn fish(at: vec2<f32>, centre: vec2<f32>, facing: f32, colour: vec3<f32>) -> vec4<f32> {
    let f = floor((at - centre) * 16.0) * vec2<f32>(facing, 1.0);
    let body = (f.x * f.x) / 12.0 + (f.y + 0.5) * (f.y + 0.5) / 3.0 < 1.0;
    let tail = f.x <= -3.0 && f.x >= -5.0 && abs(f.y + 0.5) <= -f.x - 2.5;
    if !(body || tail) {
        return vec4<f32>(0.0);
    }
    if f.x == 2.0 && f.y == 0.0 {
        return vec4<f32>(0.05, 0.05, 0.08, 1.0);
    }
    let stripe = f.x == 0.0 || f.x == -3.0;
    return vec4<f32>(select(colour, vec3<f32>(0.95), stripe && body), 1.0);
}

fn aquarium(input: WorldMaterialInput, origin: vec3<f32>, dir: vec3<f32>, depth: f32) -> vec3<f32> {
    let time = seconds(input);
    let exit = box_exit(origin, dir, vec3<f32>(0.0), vec3<f32>(1.0, 1.0, depth));
    var colour = vec3<f32>(0.0);
    let wall = origin + dir * exit;
    let px = floor(wall * 16.0);
    if wall.y < 0.01 {
        // Sand with slow caustic light.
        let caustic = sin(px.x * 0.9 + time * 2.0) + sin(px.z * 1.3 - time * 1.7);
        colour = vec3<f32>(0.86, 0.78, 0.5) * (0.75 + 0.1 * hash3(px)) + select(0.0, 0.18, caustic > 1.2);
    } else {
        colour = mix(vec3<f32>(0.1, 0.45, 0.7), vec3<f32>(0.02, 0.12, 0.3), floor(wall.y * 4.0) / 4.0);
    }
    var nearest = exit;
    // Kelp strands on two depths, swaying by row.
    let kelp_a = plane_hit(origin, dir, depth * 0.55, nearest);
    let sway_a = 0.08 * sin(kelp_a.y * 6.0 + time * 1.3);
    if kelp_a.w > 0.0 && abs(floor((kelp_a.x - 0.25 - sway_a * kelp_a.y) * 16.0)) <= 0.0 && kelp_a.y < 0.85 {
        colour = vec3<f32>(0.18, 0.55, 0.2);
        nearest = kelp_a.w;
    }
    let kelp_b = plane_hit(origin, dir, depth * 0.8, nearest);
    let sway_b = 0.08 * sin(kelp_b.y * 6.0 + time * 1.1 + 2.0);
    if kelp_b.w > 0.0 && abs(floor((kelp_b.x - 0.7 - sway_b * kelp_b.y) * 16.0)) <= 0.0 && kelp_b.y < 0.7 {
        colour = vec3<f32>(0.12, 0.45, 0.18);
        nearest = kelp_b.w;
    }
    // Bubbles rising in a column.
    let bubble_plane = plane_hit(origin, dir, depth * 0.35, nearest);
    let bubble_cell = floor(vec2<f32>(bubble_plane.x * 16.0, bubble_plane.y * 16.0 - time * 6.0));
    if bubble_plane.w > 0.0 && abs(bubble_cell.x - 12.0) < 0.5 && hash2(vec2<f32>(bubble_cell.y, 3.0)) > 0.72 {
        colour = vec3<f32>(0.8, 0.95, 1.0);
        nearest = bubble_plane.w;
    }
    // Fish at three depths, each crossing and turning back.
    let swims = array<vec3<f32>, 3>(vec3<f32>(0.3, 0.6, 7.0), vec3<f32>(0.6, 0.35, 11.0), vec3<f32>(0.85, 0.7, 5.0));
    let colours = array<vec3<f32>, 3>(vec3<f32>(1.0, 0.5, 0.1), vec3<f32>(0.2, 0.45, 1.0), vec3<f32>(1.0, 0.85, 0.2));
    let far_fish = plane_hit(origin, dir, depth * swims[2].x, nearest);
    let far_t = fract(time / swims[2].z);
    let far_x = 0.5 + 0.45 * sin(TAU * far_t);
    let far = fish(far_fish.xy, vec2<f32>(far_x, swims[2].y + 0.03 * sin(time * 2.0)), select(-1.0, 1.0, cos(TAU * far_t) > 0.0), colours[2]);
    if far_fish.w > 0.0 && far.a > 0.5 {
        colour = far.rgb;
        nearest = far_fish.w;
    }
    let mid_fish = plane_hit(origin, dir, depth * swims[1].x, nearest);
    let mid_t = fract(time / swims[1].z + 0.3);
    let mid_x = 0.5 + 0.45 * sin(TAU * mid_t);
    let mid = fish(mid_fish.xy, vec2<f32>(mid_x, swims[1].y + 0.03 * sin(time * 1.7)), select(-1.0, 1.0, cos(TAU * mid_t) > 0.0), colours[1]);
    if mid_fish.w > 0.0 && mid.a > 0.5 {
        colour = mid.rgb;
        nearest = mid_fish.w;
    }
    let near_fish = plane_hit(origin, dir, depth * swims[0].x, nearest);
    let near_t = fract(time / swims[0].z + 0.6);
    let near_x = 0.5 + 0.45 * sin(TAU * near_t);
    let near = fish(near_fish.xy, vec2<f32>(near_x, swims[0].y + 0.03 * sin(time * 2.3)), select(-1.0, 1.0, cos(TAU * near_t) > 0.0), colours[0]);
    if near_fish.w > 0.0 && near.a > 0.5 {
        colour = near.rgb;
        nearest = near_fish.w;
    }
    // Deeper water swallows colour, in four steps.
    let murk = floor(clamp(nearest * dir.z / depth, 0.0, 1.0) * 4.0) / 4.0;
    return mix(colour, vec3<f32>(0.02, 0.15, 0.35), murk * 0.6);
}

fn mineshaft(input: WorldMaterialInput, origin: vec3<f32>, dir: vec3<f32>, depth: f32) -> vec3<f32> {
    let time = seconds(input);
    let exit = box_exit(origin, dir, vec3<f32>(0.0), vec3<f32>(1.0, 1.0, depth));
    let p = origin + dir * exit;
    let px = floor(p * 16.0);
    var colour = vec3<f32>(0.42, 0.42, 0.44) * (0.7 + 0.35 * hash3(px));
    // Timber frames every two blocks: posts at the sides and a beam overhead.
    let frame = fract(p.z / 2.0) < 0.12;
    let post = p.x < 0.01 || p.x > 0.99;
    let beam = p.y > 0.99;
    if frame && (post || beam) {
        colour = vec3<f32>(0.5, 0.34, 0.18) * (0.8 + 0.2 * hash3(px));
    }
    // Rails on the floor: two lines over sleepers.
    if p.y < 0.01 {
        colour = vec3<f32>(0.33, 0.27, 0.2) * (0.7 + 0.3 * hash3(px));
        if fract(p.z * 2.0) < 0.25 && p.x > 0.15 && p.x < 0.85 {
            colour = vec3<f32>(0.45, 0.3, 0.16);
        }
        if abs(px.x - 4.0) < 0.5 || abs(px.x - 11.0) < 0.5 {
            colour = vec3<f32>(0.65, 0.65, 0.68);
        }
    }
    // The torch on the far wall, flickering, lights everything by distance.
    let torch = vec3<f32>(0.5, 0.45, depth);
    let flicker = 0.85 + 0.15 * sin(time * 23.0) * sin(time * 7.3);
    let at_torch = p.z > depth - 0.01 && abs(px.x - 8.0) < 0.5 && px.y >= 6.0 && px.y <= 8.0;
    if at_torch {
        return select(vec3<f32>(0.4, 0.26, 0.12), vec3<f32>(1.0, 0.85, 0.35) * flicker, px.y == 8.0);
    }
    let reach = length(p - torch);
    let light = clamp(1.6 / (1.0 + reach * reach * 0.12), 0.0, 1.0) * flicker;
    let entrance = clamp(1.0 - p.z * 0.5, 0.0, 1.0) * 0.5;
    let lit = floor((light + entrance) * 5.0) / 5.0;
    return colour * vec3<f32>(1.0, 0.8, 0.55) * lit;
}

fn shade(input: WorldMaterialInput) -> Surface {
    let local = local_of(input);
    let frame = frame_of(input.normal);
    let dir = -input.view;
    let origin = vec3<f32>(dot(local, frame.u), dot(local, frame.v), 0.001);
    let room_dir = vec3<f32>(dot(dir, frame.u), dot(dir, frame.v), max(dot(dir, frame.w), 0.02));
    let depth = max(param(input, 1u), 1.0);
    let uv = origin.xy;
    // A one-texel stone frame keeps the block reading as part of its wall.
    if min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y)) < 1.0 / 16.0 {
        return Surface(vec3<f32>(0.5, 0.5, 0.52) * (0.8 + 0.2 * hash2(floor(uv * 16.0))), 1.0, vec3<f32>(0.0));
    }
    if param(input, 0u) < 0.5 {
        let water = aquarium(input, origin, room_dir, depth);
        return Surface(water, 1.0, water * 0.35);
    }
    let shaft = mineshaft(input, origin, room_dir, depth);
    return Surface(shaft, 1.0, shaft * 0.6);
}
