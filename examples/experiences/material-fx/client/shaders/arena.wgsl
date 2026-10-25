// Arena floor: dark riveted plates that carry pixel energy. A shockwave ring races out from the
// impact point the server names and fades; in heartbeat mode the floor beats in double pulses
// that quicken as the boss's health falls. The server sends the centre and start time on the
// session clock as parameters, so nothing recompiles.
// params 0-1: impact x and z; param 2: impact start in session seconds; param 3: mode (0
// shockwave, 1 heartbeat); param 4: boss health (0 to 1); params 5-6: heart x and z.

fn plates(texel: vec2<f32>) -> vec3<f32> {
    let inner = (texel % 8.0 + 8.0) % 8.0;
    let seam = inner.x == 0.0 || inner.y == 0.0;
    let rivet = (inner.x == 1.0 || inner.x == 7.0) && (inner.y == 1.0 || inner.y == 7.0);
    let grain = 0.85 + 0.15 * hash2(texel);
    var colour = vec3<f32>(0.16, 0.17, 0.2) * grain;
    colour = select(colour, vec3<f32>(0.07, 0.07, 0.09), seam);
    return select(colour, vec3<f32>(0.42, 0.44, 0.5), rivet);
}

fn shade(input: WorldMaterialInput) -> Surface {
    let texel = floor(input.world_position.xz * 16.0);
    let at = (texel + 0.5) / 16.0;
    let base = plates(texel);
    if param(input, 3u) > 0.5 {
        // Heartbeat: two pulses per beat, the beat faster as health falls.
        let health = clamp(param(input, 4u), 0.0, 1.0);
        let period = mix(0.45, 1.4, health);
        let beat = fract(seconds(input) / period);
        let pulse = max(1.0 - abs(beat - 0.1) * 12.0, 0.0) + 0.6 * max(1.0 - abs(beat - 0.32) * 12.0, 0.0);
        let reach = distance(at, vec2<f32>(param(input, 5u), param(input, 6u)));
        let wave = fract(reach * 0.25 - beat * 2.0);
        let ring = select(0.0, 1.0, wave < 0.08) * pulse;
        let veins = select(0.0, 0.5, ((texel.x + texel.y) % 8.0 + 8.0) % 8.0 == 0.0) * pulse;
        let glow = floor(clamp(ring + veins, 0.0, 1.0) * 4.0) / 4.0;
        return Surface(base, 1.0, vec3<f32>(1.0, 0.1, 0.15) * glow * 2.0);
    }
    let age = since(input, param(input, 2u));
    let radius = age * 9.0;
    let reach = distance(at, vec2<f32>(param(input, 0u), param(input, 1u)));
    let band = abs(reach - radius) * 16.0;
    let fade = clamp(1.0 - age / 2.5, 0.0, 1.0);
    let core = select(0.0, 1.0, band < 1.0);
    let halo = select(0.0, 0.45, band < 3.0 && reach < radius);
    let sparks = select(0.0, 0.8, band < 6.0 && hash3(vec3<f32>(texel, floor(age * 20.0))) > 0.93);
    let energy = floor(clamp(core + halo + sparks, 0.0, 1.0) * fade * 4.0) / 4.0;
    let colour = mix(vec3<f32>(0.2, 0.85, 1.0), vec3<f32>(1.0), core);
    return Surface(base, 1.0, colour * energy * 3.0);
}
