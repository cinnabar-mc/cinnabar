// Corruption: a purple pixel infection that creeps across bound stone block by block. Each block
// starts turning when the front from the server's source point reaches it, and the infection
// spreads across its texels from the side facing the source, with a glowing leading edge and
// pulsing veins behind it. The server sends the source and start time; a start of -1 keeps the
// stone clean.
// params 0-2: the source; param 3: start in session seconds, or below 0 for none; param 4:
// speed in blocks per second.

fn shade(input: WorldMaterialInput) -> Surface {
    let texel = floor((input.world_position - input.normal * 0.001) * 16.0);
    let h = hash3(floor(texel / 2.0)) * 0.6 + hash3(texel) * 0.4;
    let stone = vec3<f32>(0.47, 0.47, 0.49) * (0.72 + 0.4 * h);
    let start = param(input, 3u);
    if start < 0.0 {
        return Surface(stone, 1.0, vec3<f32>(0.0));
    }
    let time = seconds(input);
    let age = since(input, start);
    let source = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let block = cell_of(input);
    // The block's turn comes when the front reaches its centre; inside it the infection runs
    // across the texels, from the side facing the source, with a ragged edge.
    let block_time = distance(block + 0.5, source) / max(param(input, 4u), 0.05);
    let towards = normalize(source - (block + 0.5));
    let texel_offset = dot((texel + 0.5) / 16.0 - (block + 0.5), -towards) + 0.5;
    let ragged = hash3(texel) * 0.35;
    let progress = (age - block_time) * 0.8 - texel_offset - ragged;
    if progress < 0.0 {
        return Surface(stone, 1.0, vec3<f32>(0.0));
    }
    let edge = progress < 0.12;
    let vein = hash3(floor(texel / 3.0) + 11.0) > 0.6 && hash3(texel + 2.0) > 0.4;
    let beat = 0.6 + 0.4 * sin(time * 3.0 + hash3(block) * 6.0);
    let rot = posterize(mix(vec3<f32>(0.22, 0.05, 0.3), vec3<f32>(0.4, 0.1, 0.5), h), 5.0);
    if edge {
        return Surface(vec3<f32>(0.7, 0.25, 0.9), 1.0, vec3<f32>(0.9, 0.2, 1.3));
    }
    let glow = select(vec3<f32>(0.0), vec3<f32>(0.8, 0.15, 1.0) * beat * 1.5, vein);
    return Surface(rot, 1.0, glow);
}
