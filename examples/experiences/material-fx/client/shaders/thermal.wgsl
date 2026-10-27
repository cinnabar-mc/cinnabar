// Thermal vision: the mob in a heat-map palette, hottest at its core and head, coolest at its
// edges, in six fixed palette steps on the skin's 64-texel grid, with a slow shimmer and a
// scanline. The skin's coverage is kept.
// param 0: body heat (0 to 1); param 1: shimmer strength.

fn palette(heat: f32) -> vec3<f32> {
    let step = floor(clamp(heat, 0.0, 0.999) * 6.0);
    var colours = array<vec3<f32>, 6>(
        vec3<f32>(0.05, 0.02, 0.2), vec3<f32>(0.35, 0.05, 0.55), vec3<f32>(0.85, 0.1, 0.35),
        vec3<f32>(1.0, 0.45, 0.05), vec3<f32>(1.0, 0.85, 0.15), vec3<f32>(1.0, 1.0, 0.9));
    return colours[u32(step)];
}

fn shade(input: WorldMaterialInput) -> Surface {
    if input.base.a < 0.5 {
        return Surface(vec3<f32>(0.0), 0.0, vec3<f32>(0.0));
    }
    let time = seconds(input);
    let texel = floor(input.uv * 64.0);
    let facing = abs(dot(input.normal, input.view));
    let luminance = dot(input.base.rgb, vec3<f32>(0.3, 0.6, 0.1));
    let shimmer = (hash3(vec3<f32>(texel, floor(time * 4.0))) - 0.5) * clamp(param(input, 1u), 0.0, 1.0) * 0.08;
    let heat = clamp(param(input, 0u), 0.0, 1.0) * (0.2 + 0.65 * facing) + luminance * 0.2 + shimmer;
    let scan = select(1.0, 1.25, abs(fract(input.world_position.y * 4.0 - time * 0.5) - 0.5) < 0.04);
    let colour = palette(heat) * scan;
    return Surface(colour * 0.3, 1.0, colour * 1.2);
}
