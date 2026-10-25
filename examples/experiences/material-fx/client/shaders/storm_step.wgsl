// The storm's particles, one per cell: x, y, z from the storm core and the life left in w. Each
// frame a particle turns about the core, drifts up and down and ages; a dead one is reborn on a
// ring around the core. After a burst they spin faster and fly outward.
// param 0: a burst's start in session seconds; param 1: frames per second the step assumes.

fn storm_hash(n: f32) -> f32 {
    return fract(sin(n * 12.9898 + 78.233) * 43758.5453);
}

fn step(input: WorldCellInput) -> vec4<f32> {
    let previous = cell(input.index);
    let seconds = input.phase * 4096.0;
    let dt = 1.0 / max(compute_param(1u), 10.0);
    let age = fract((seconds - compute_param(0u)) / 4096.0 + 1.0) * 4096.0;
    let burst = select(0.0, 1.0 - age / 3.0, age < 3.0);
    let seed = f32(input.index) * 1.618 + floor(seconds * 2.0);
    if previous.w <= 0.0 {
        let angle = storm_hash(seed) * 6.2831853;
        let radius = 0.8 + storm_hash(seed + 1.0) * 1.6;
        return vec4<f32>(cos(angle) * radius, storm_hash(seed + 2.0) * 2.0 - 1.0, sin(angle) * radius, 0.5 + storm_hash(seed + 3.0) * 0.5);
    }
    let p = previous.xyz;
    let turn = (1.2 + burst * 4.0) * dt;
    let spun = vec3<f32>(p.x * cos(turn) - p.z * sin(turn), p.y + sin(seconds * 3.0 + f32(input.index)) * dt * 0.8, p.x * sin(turn) + p.z * cos(turn));
    let outward = normalize(vec3<f32>(p.x, 0.0, p.z) + vec3<f32>(1e-4)) * burst * dt * 3.0;
    return vec4<f32>(spun + outward, previous.w - dt * (0.35 + burst));
}
