// Waving grass: a grass block whose top sways in the wind, whole texels at a time. Its top
// corners move with a gust that rolls across the field, the bottom stays put, so the blades on
// the top face lean and the sides shear; neighbours share their corners, so the field moves as
// one sheet without cracks. The top is a pixel lawn with blades and flowers.
// params 0-1: wind strength (0 to 1) and gust speed; params 2-4: the centre of the field's top;
// param 5: facing in quarter turns from south.

// Half the field's size in blocks: across, then ahead, as the gallery lays it out.
const FIELD_HALF: vec2<f32> = vec2<f32>(3.5, 2.5);

// How far, from 0 at the field's edge to 1 a block in, `p` may sway: the edge corners stay
// on the ground blocks around the field, so its sides never pull away and open gaps.
fn sway_room(p: vec3<f32>) -> f32 {
    let centre = vec3<f32>(parameter(2u), parameter(3u), parameter(4u));
    let angle = floor(parameter(5u) + 0.5) * TAU / 4.0;
    let rel = p - centre;
    let across = abs(dot(rel, vec3<f32>(cos(angle), 0.0, sin(angle))));
    let ahead = abs(dot(rel, vec3<f32>(-sin(angle), 0.0, cos(angle))));
    return clamp(min(FIELD_HALF.x - across, FIELD_HALF.y - ahead), 0.0, 1.0);
}

fn displace(input: WorldVertexInput) -> vec3<f32> {
    let p = input.world_position;
    // Only the top corners move; the bottoms stay on the ground.
    if p.y < parameter(3u) - 0.5 {
        return vec3<f32>(0.0);
    }
    let time = seconds_at(input.phase) * max(parameter(1u), 0.1);
    let gust = sin(time + p.x * 0.6 + p.z * 0.35) * 0.6 + sin(time * 1.7 + p.x * 1.3) * 0.4;
    let sway = gust * parameter(0u) * 0.25 * sway_room(p);
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
