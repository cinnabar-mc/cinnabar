// Breathing ore. Mode 0, diamond: pixel stone with diamond specks that each catch the light at
// their own angle, so glints wink on and off as the viewer moves. Mode 1, redstone: veins laid
// through world space run on across neighbouring blocks, and pulses of light travel along them.
// Both snap to the 16-texel grid of each face.
// param 0: mode; param 1: pulse speed.

// Pixel stone at texel `t` (world texel coordinates).
fn stone(t: vec3<f32>) -> vec3<f32> {
    let h = hash3(floor(t / 2.0)) * 0.6 + hash3(t) * 0.4;
    return vec3<f32>(0.47, 0.47, 0.49) * (0.72 + 0.4 * h);
}

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let texel = floor((input.world_position - input.normal * 0.001) * 16.0);
    let base = stone(texel);
    if param(input, 0u) < 0.5 {
        // Specks cluster in 3x3 texel cells; each has a facet direction it glints towards.
        let cluster = floor(texel / 3.0);
        let seed = hash3(cluster);
        let speck = seed > 0.78 && hash3(texel) > 0.35;
        if !speck {
            return Surface(base, 1.0, vec3<f32>(0.0));
        }
        let facet = normalize(vec3<f32>(hash3(cluster + 1.0), hash3(cluster + 2.0), hash3(cluster + 3.0)) - 0.5 + input.normal * 0.8);
        let mirror = reflect(-input.view, input.normal);
        let caught = pow(max(dot(mirror, facet), 0.0), 24.0);
        let glint = floor(caught * 3.0 + 0.5) / 3.0;
        let diamond = select(vec3<f32>(0.36, 0.86, 0.85), vec3<f32>(0.6, 1.0, 0.98), hash3(texel + 5.0) > 0.6);
        return Surface(diamond, 1.0, diamond * glint * 3.0 + vec3<f32>(glint * 2.0));
    }
    // Redstone veins: the zero set of a world-space field, two texels wide, continuous across
    // blocks. Pulses ride the field's other coordinate.
    let p = (texel + 0.5) / 16.0;
    let q = p * 3.0;
    let field = sin(q.x * 2.3 + sin(q.y * 1.7)) + sin(q.z * 2.1 + sin(q.x * 1.3)) + sin(q.y * 2.7 + q.z * 0.6);
    let vein = abs(field) < 0.3;
    if !vein {
        return Surface(base, 1.0, vec3<f32>(0.0));
    }
    let along = p.x * 0.9 + p.y * 1.3 + p.z * 0.7;
    let pulse = fract(along * 0.5 - time * max(param(input, 1u), 0.1) * 0.25);
    let bright = select(0.25, 1.0, pulse < 0.12) + select(0.0, 0.4, pulse < 0.2);
    let level = floor(bright * 3.0 + 0.5) / 3.0;
    let red = vec3<f32>(0.75, 0.04, 0.02);
    return Surface(red, 1.0, vec3<f32>(1.0, 0.1, 0.05) * level * 2.5);
}
