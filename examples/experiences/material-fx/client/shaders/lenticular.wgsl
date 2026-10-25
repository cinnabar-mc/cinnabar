// Lenticular mob: like a ridged print, its skin shows one of three faces depending on the angle
// it is seen from. Texture 0 holds three 64x64 skins side by side; the view's sideways angle to
// each surface picks one, a pixel ridge pattern shimmers at the boundaries, and the skin's
// coverage is kept.
// param 0: angle per face in radians.

fn shade(input: WorldMaterialInput) -> Surface {
    if input.base.a < 0.5 {
        return Surface(vec3<f32>(0.0), 0.0, vec3<f32>(0.0));
    }
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let side = cross(up, input.normal);
    let across = select(normalize(side), vec3<f32>(1.0, 0.0, 0.0), length(side) < 0.1);
    let angle = atan2(dot(input.view, across), max(dot(input.view, input.normal), 0.05));
    let width = max(param(input, 0u), 0.1);
    let texel = floor(input.uv * 64.0);
    // Each texel column is a lenticule: its own slight offset makes the switch sweep across.
    let ridge = (fract(texel.x * 0.5) - 0.25) * 0.15;
    let pick = clamp(floor((angle + ridge) / width + 1.5), 0.0, 2.0);
    let skin = texture(0u, vec2<f32>((pick + (texel.x + 0.5) / 64.0) / 3.0, (texel.y + 0.5) / 64.0));
    let edge = abs(fract((angle + ridge) / width + 1.5) - 0.5) > 0.42;
    let sheen = select(0.0, 0.25, edge);
    let colour = pow(skin.rgb, vec3<f32>(0.4545));
    return Surface(colour, 1.0, vec3<f32>(sheen) * colour);
}
