// Waving grass: a grass block whose top sways in the wind, whole texels at a time. Its top
// corners move with a gust that rolls across the field, the bottom stays put, so the blades on
// the top face lean and the sides shear; neighbours share their corners, so the field moves as
// one sheet without cracks. The top is a pixel lawn with blades and flowers.
// param 0: wind strength (0 to 1); param 1: gust speed; param 2: the height of the field's top.

fn displace(input: WorldVertexInput) -> vec3<f32> {
    let p = input.world_position;
    if p.y < parameter(2u) - 0.5 {
        return vec3<f32>(0.0);
    }
    let time = seconds_at(input.phase) * max(parameter(1u), 0.1);
    let gust = sin(time + p.x * 0.6 + p.z * 0.35) * 0.6 + sin(time * 1.7 + p.x * 1.3) * 0.4;
    let sway = gust * parameter(0u) * 0.25;
    return to_texels(vec3<f32>(sway, 0.0, sway * 0.4));
}

fn shade(input: WorldMaterialInput) -> Surface {
    let local = local_of(input);
    let texel = floor(face_uv(local, input.normal) * TEXELS);
    let h = hash2(texel + cell_of(input).xz * 31.0);
    let grass = posterize(vec3<f32>(0.28, 0.62, 0.22) * (0.75 + 0.45 * h), 6.0);
    if input.normal.y > 0.5 {
        let flower = h > 0.97;
        let colour = select(grass, vec3<f32>(0.95, 0.85, 0.2), flower);
        return Surface(colour, 1.0, vec3<f32>(0.0));
    }
    let dirt = posterize(vec3<f32>(0.45, 0.31, 0.2) * (0.8 + 0.3 * h), 6.0);
    // A grass fringe hangs down the sides, longer in places.
    let fringe = texel.y >= 13.0 - floor(h * 3.0);
    return Surface(select(dirt, grass, fringe), 1.0, vec3<f32>(0.0));
}
