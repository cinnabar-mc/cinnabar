// Shatter mob: every few seconds it bursts into its pixels and pulls itself back together.
// Each face flies out along its normal and a scattered direction of its own, rounded to whole
// texels, while its texels drop out at random with a white-hot spark; then everything returns.
// A server command starts a burst at once.
// param 0: cycle length in seconds; param 1: a burst's start in session seconds, or below 0.

fn burst(phase: f32) -> f32 {
    let cycle = max(parameter(0u), 1.0);
    let started = since_at(phase, parameter(1u));
    var t = fract(seconds_at(phase) / cycle);
    if parameter(1u) >= 0.0 && started < cycle {
        t = started / cycle;
    }
    // Out quickly, hang, come back.
    return smoothstep(0.0, 0.25, t) * (1.0 - smoothstep(0.55, 0.9, t));
}

fn displace(input: WorldVertexInput) -> vec3<f32> {
    let amount = burst(input.phase);
    let face = floor(input.world_position * 4.0) + input.normal * 7.0;
    let scatter = vec3<f32>(hash3(face) - 0.5, hash3(face + 3.0), hash3(face + 7.0) - 0.5);
    return to_texels((input.normal * 0.6 + scatter) * amount * 1.5);
}

fn shade(input: WorldMaterialInput) -> Surface {
    let amount = burst(input.phase);
    let texel = floor(input.uv * 64.0);
    let skin = posterize(input.base.rgb, 6.0);
    let gone = hash2(texel) < amount * 0.85;
    let edge = hash2(texel) < amount * 0.85 + 0.06 && !gone && amount > 0.0;
    let spark = select(vec3<f32>(0.0), vec3<f32>(1.0, 0.9, 0.7) * 2.0, edge);
    return Surface(skin, select(1.0, 0.0, gone), spark);
}
