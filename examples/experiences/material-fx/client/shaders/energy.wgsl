// Molten energy flowing down a block face: two scrolling sine fields interfere into veins that
// run from dark crimson through orange to white-hot, and the hot veins emit light, so the block
// glows in the dark. The flow is continuous across neighbouring blocks.
// param 0: flow speed in cycles per 4 s; param 1: emission strength.
fn shade(input: WorldMaterialInput) -> Surface {
    let p = input.world_position;
    let flow = fract(input.phase * 1024.0) * param(input, 0u);
    let a = sin(6.2831853 * (p.x * 0.7 + p.z * 0.4 + (p.y + flow) * 1.3));
    let b = sin(6.2831853 * (p.x * -0.5 + p.z * 0.9 + (p.y + flow * 0.6) * 0.8) + 1.7);
    let heat = clamp(0.5 + 0.5 * a * b + 0.25 * sin(6.2831853 * (p.y + flow) * 2.1), 0.0, 1.0);
    let crimson = vec3<f32>(0.25, 0.02, 0.01);
    let orange = vec3<f32>(1.0, 0.35, 0.02);
    let white = vec3<f32>(1.0, 0.92, 0.6);
    let colour = mix(mix(crimson, orange, smoothstep(0.2, 0.6, heat)), white, smoothstep(0.75, 1.0, heat));
    let glow = smoothstep(0.45, 1.0, heat) * param(input, 1u);
    return Surface(colour, 1.0, colour * glow);
}
