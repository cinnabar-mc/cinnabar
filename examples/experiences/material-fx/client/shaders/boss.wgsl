// Boss: a dark armoured knight with burning eyes and glowing runes on its plate. Its eyes and
// runes are emissive, so bloom spreads them; with the outline effect a pixel-wide red outline
// rings what can be seen of it, beating with the heartbeat.
// param 0: beats per second; param 1: the beat's phase at session second zero.

fn shade(input: WorldMaterialInput) -> Surface {
    let texel = floor(input.uv * 64.0);
    let plate = posterize(vec3<f32>(0.16, 0.15, 0.2) * (0.8 + 0.4 * hash2(floor(texel / 4.0))), 6.0);
    let rivet = (u32(texel.x) % 8u == 3u) && (u32(texel.y) % 8u == 3u);
    let rune = hash2(floor(texel / 2.0) + 40.0) > 0.92;
    let eyes = texel.y >= 6.0 && texel.y <= 7.0 && (texel.x == 10.0 || texel.x == 13.0);
    let beat = fract(seconds(input) * param(input, 0u) + param(input, 1u));
    let pulse = 0.6 + 0.4 * max(1.0 - abs(beat - 0.1) * 10.0, 0.0);
    var glow = vec3<f32>(0.0);
    if eyes {
        glow = vec3<f32>(3.0, 0.6, 0.1);
    } else if rune {
        glow = vec3<f32>(1.2, 0.15, 0.1) * pulse;
    }
    let colour = select(plate, vec3<f32>(0.45, 0.45, 0.5), rivet);
    return Surface(colour, 1.0, glow);
}

fn outline(input: WorldMaterialInput) -> vec3<f32> {
    let beat = fract(seconds(input) * param(input, 0u) + param(input, 1u));
    let pulse = 0.5 + 0.5 * max(1.0 - abs(beat - 0.1) * 8.0, 0.0);
    return vec3<f32>(1.0, 0.1, 0.05) * (1.0 + pulse * 2.0);
}
