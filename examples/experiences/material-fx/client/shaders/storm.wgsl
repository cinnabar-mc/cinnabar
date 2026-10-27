// Storm core: a block of churning storm cloud with lightning veins, at the heart of a GPU
// pixel particle storm. The particles' positions live in the bundle's compute cells, which the
// storm step moves each frame: they orbit the core, rise and fall back, and race out after a
// burst. Each particle is one or two texels of electric blue, white-hot when young.
// param 0: a burst's start in session seconds.

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let texel = floor(face_uv(local_of(input), input.normal) * TEXELS);
    let swirl = hash2(floor((texel + vec2<f32>(floor(time * 3.0), 0.0)) / 2.0));
    let cloud = posterize(vec3<f32>(0.2, 0.22, 0.32) * (0.6 + 0.6 * swirl), 5.0);
    let bolt = hash2(vec2<f32>(texel.x, floor(time * 8.0))) > 0.95 && hash2(texel) > 0.5;
    let glow = select(vec3<f32>(0.0), vec3<f32>(0.6, 0.8, 1.6), bolt);
    return Surface(cloud, 1.0, glow);
}

fn particle(input: WorldParticleInput) -> WorldParticle {
    let c = cell(input.index);
    let life = c.w;
    let size = select(1.0, 2.0, life > 0.8);
    let colour = mix(vec3<f32>(0.25, 0.5, 1.6), vec3<f32>(1.6, 1.8, 2.0), clamp(life * 1.5 - 0.5, 0.0, 1.0));
    return WorldParticle(to_texels(c.xyz), select(0.0, size, life > 0.0), colour, 1.0);
}
