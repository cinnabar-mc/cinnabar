// Carved rune block: a dressed stone face with runes cut deep into it, found per pixel by
// stepping the view ray down through a height map (texture 0, 16x16, alpha 1 = surface, 0 =
// the bottom of a cut) in twelve steps, each landing on a texel, so the relief stays blocky but
// its walls shift and occlude with the viewing angle. The bottoms of the cuts glow and pulse.
// param 0: cut depth in blocks; params 1-3: glow colour.

fn height(uv: vec2<f32>) -> f32 {
    return texture(0u, (floor(fract(uv) * 16.0) + 0.5) / 16.0).a;
}

fn descend(state: vec4<f32>, step: vec3<f32>) -> vec4<f32> {
    if state.w > 0.0 {
        return state;
    }
    let next = state.xyz + step;
    let surface = height(next.xy);
    return vec4<f32>(next, select(0.0, 1.0, next.z <= surface));
}

fn shade(input: WorldMaterialInput) -> Surface {
    let local = local_of(input);
    let uv = face_uv(local, input.normal);
    let n = abs(input.normal);
    var across = vec3<f32>(1.0, 0.0, 0.0);
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if n.y > 0.5 {
        up = vec3<f32>(0.0, 0.0, 1.0);
    } else if n.x > 0.5 {
        across = vec3<f32>(0.0, 0.0, 1.0);
    }
    let d = -input.view;
    let depth = max(param(input, 0u), 0.02);
    let inward = max(-dot(d, input.normal), 0.08);
    // Per step: one twelfth of the depth down, and the matching slide across the face.
    let slide = vec2<f32>(dot(d, across), dot(d, up)) / inward * depth / 12.0;
    let step = vec3<f32>(slide, -1.0 / 12.0);
    var state = vec4<f32>(uv, 1.0, select(0.0, 1.0, height(uv) >= 1.0));
    state = descend(descend(descend(descend(state, step), step), step), step);
    state = descend(descend(descend(descend(state, step), step), step), step);
    state = descend(descend(descend(descend(state, step), step), step), step);
    let texel = floor(fract(state.xy) * 16.0);
    let level = state.z;
    let stone = vec3<f32>(0.62, 0.6, 0.56) * (0.8 + 0.2 * hash2(texel)) * (0.55 + 0.45 * level);
    let time = seconds(input);
    let pulse = 0.6 + 0.4 * sin(time * 2.0 - (texel.x + texel.y) * 0.3);
    let glow = vec3<f32>(param(input, 1u), param(input, 2u), param(input, 3u));
    let cut = level < 0.3;
    return Surface(stone, 1.0, select(vec3<f32>(0.0), glow * pulse * 2.0, cut));
}
