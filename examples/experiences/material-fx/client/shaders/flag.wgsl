// Flag: a 3x2 block cloth rippling in the wind off a pole on its right. Every vertex moves out
// of the flag's plane by a travelling wave that grows with its distance from the pole, rounded
// to whole texels, so the cloth folds in pixel steps. The cloth is a two-colour banner with a
// star near the pole.
// params 0-2: the cloth's centre; param 3: facing in quarter turns from south; param 4: wind
// strength (0 to 1).

// Across the flag, away from the pole (the gallery's left), and out of its plane.
fn flag_frame(quarter: f32) -> mat2x3<f32> {
    let angle = floor(quarter + 0.5) * TAU / 4.0;
    return mat2x3<f32>(vec3<f32>(cos(angle), 0.0, sin(angle)), vec3<f32>(-sin(angle), 0.0, cos(angle)));
}

// Blocks from the pole across the flag, and up from its bottom edge.
fn flag_coordinates(p: vec3<f32>, centre: vec3<f32>, quarter: f32) -> vec2<f32> {
    let rel = p - centre;
    return vec2<f32>(dot(rel, flag_frame(quarter)[0]) + 1.5, rel.y + 1.0);
}

fn displace(input: WorldVertexInput) -> vec3<f32> {
    let centre = vec3<f32>(parameter(0u), parameter(1u), parameter(2u));
    let away = clamp(flag_coordinates(input.world_position, centre, parameter(3u)).x, 0.0, 3.0);
    let time = seconds_at(input.phase);
    let wave = sin(away * 2.2 - time * 4.0) * (0.15 + 0.05 * sin(time * 0.7));
    return to_texels(flag_frame(parameter(3u))[1] * wave * away * parameter(4u));
}

fn shade(input: WorldMaterialInput) -> Surface {
    let centre = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let along = flag_coordinates(input.world_position - input.normal * 0.001, centre, param(input, 3u));
    let texel = floor(along * TEXELS);
    // Crimson above, gold below, with a white star near the pole.
    var colour = select(vec3<f32>(0.85, 0.65, 0.1), vec3<f32>(0.7, 0.08, 0.12), texel.y >= 16.0);
    let star = texel - vec2<f32>(8.0, 22.0);
    if abs(star.x) + abs(star.y) * 0.6 < 4.0 && abs(star.x) * 0.6 + abs(star.y) < 4.0 {
        colour = vec3<f32>(0.95, 0.95, 0.9);
    }
    let weave = select(0.92, 1.0, (u32(abs(texel.x + texel.y)) % 2u) == 0u);
    return Surface(colour * weave, 1.0, vec3<f32>(0.0));
}
