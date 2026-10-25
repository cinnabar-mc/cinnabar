// The wall fire's cellular automaton, one cell per texel of a FIRE_WIDTH by FIRE_HEIGHT wall:
// x holds heat, y whether the texel has burnt. Heat rises from the texel below and its
// diagonals and cools as it climbs; a texel next to a burning one catches fire at random, so
// flame spreads sideways along the bottom and climbs. Igniting lights the bottom row's left
// end; extinguishing cools everything.
// param 0: ignition's start in session seconds, or below 0 to put it out; param 1: width in
// texels; param 2: random seed.

fn fire_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn fire_at(x: i32, y: i32, width: i32, height: i32) -> vec4<f32> {
    if x < 0 || y < 0 || x >= width || y >= height {
        return vec4<f32>(0.0);
    }
    return cell(u32(y * width + x));
}

fn step(input: WorldCellInput) -> vec4<f32> {
    let width = i32(max(compute_param(1u), 1.0));
    let height = i32(input.count) / width;
    let x = i32(input.index) % width;
    let y = i32(input.index) / width;
    let here = cell(input.index);
    if compute_param(0u) < 0.0 {
        return vec4<f32>(0.0, here.y, 0.0, 0.0);
    }
    let seconds = input.phase * 4096.0;
    let age = fract((seconds - compute_param(0u)) / 4096.0 + 1.0) * 4096.0;
    let noise = fire_hash(vec2<f32>(f32(input.index), floor(seconds * 30.0) + compute_param(2u)));
    // Ignition at the bottom left, spreading sideways along the bottom row.
    let left = fire_at(x - 1, y, width, height).x;
    let right = fire_at(x + 1, y, width, height).x;
    var fuel = 0.0;
    if y == 0 {
        if x == 0 && age < 2.0 {
            fuel = 1.0;
        } else if here.x > 0.3 || ((left > 0.5 || right > 0.5) && noise > 0.93) {
            fuel = 0.9 + noise * 0.1;
        }
    }
    let below = fire_at(x, y - 1, width, height).x;
    let diagonal = max(fire_at(x - 1, y - 1, width, height).x, fire_at(x + 1, y - 1, width, height).x);
    let rising = max(below * (0.9 - noise * 0.12), diagonal * 0.7);
    let heat = max(max(fuel, rising), here.x * 0.6) - 0.015;
    let burnt = select(here.y, 1.0, heat > 0.6);
    return vec4<f32>(clamp(heat, 0.0, 1.0), burnt, 0.0, 0.0);
}
