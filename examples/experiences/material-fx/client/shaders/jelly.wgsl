// Jelly mob: a wobbling green slime of pixel jelly. It breathes slowly, and a hit makes it
// shake: every vertex swings along its normal and squashes towards the ground, fading out over
// a second, in whole texels. Its translucent body shows darker depth and bright highlights.
// params 0-2: the last hit; param 3: the hit's start in session seconds.

fn displace(input: WorldVertexInput) -> vec3<f32> {
    let time = seconds_at(input.phase);
    let age = since_at(input.phase, parameter(3u));
    let shake = sin(age * 22.0) * exp(-age * 3.5) * select(0.0, 1.0, age < 3.0);
    let breathe = sin(time * 2.0) * 0.03;
    let height = input.world_position.y - parameter(1u);
    let squash = vec3<f32>(0.0, -abs(shake) * 0.25 * clamp(height, 0.0, 2.0), 0.0);
    return to_texels(input.normal * (shake * 0.18 + breathe) + squash);
}

fn shade(input: WorldMaterialInput) -> Surface {
    let texel = floor(input.uv * 64.0);
    let facing = clamp(dot(input.normal, input.view), 0.0, 1.0);
    let depth = posterize(vec3<f32>(0.2, 0.75, 0.3) * (0.45 + 0.55 * facing), 5.0);
    let highlight = hash2(floor(texel / 3.0)) > 0.93 && facing > 0.6;
    let core = hash2(texel) > 0.985;
    let colour = select(depth, vec3<f32>(0.85, 1.0, 0.85), highlight);
    let glow = select(vec3<f32>(0.0), vec3<f32>(0.2, 0.9, 0.3), core);
    return Surface(colour, 0.78, glow);
}
