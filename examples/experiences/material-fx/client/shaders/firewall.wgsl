// Pixel fire: a cellular automaton spreads flame across a stone wall. The bundle's compute
// cells hold the heat of every texel of the wall (FIRE_WIDTH by FIRE_HEIGHT), which the fire
// step updates each frame: heat rises and cools, and burning texels set their neighbours
// alight. Each texel of the wall reads its own cell and draws a five-colour flame palette,
// emissive where it burns.
// params 0-2: the wall's centre; param 3: facing in quarter turns from south; params 4-5: the
// wall's width and height in blocks.

fn shade(input: WorldMaterialInput) -> Surface {
    let centre = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let angle = floor(param(input, 3u) + 0.5) * TAU / 4.0;
    // From the wall's right end as the player sees it, towards its left.
    let across = vec3<f32>(cos(angle), 0.0, sin(angle));
    let blocks = vec2<f32>(max(param(input, 4u), 1.0), max(param(input, 5u), 1.0));
    let rel = input.world_position - centre - input.normal * 0.001;
    let width = u32(blocks.x * TEXELS);
    let height = u32(blocks.y * TEXELS);
    let x = u32(clamp(floor((dot(rel, across) + blocks.x * 0.5) * TEXELS), 0.0, f32(width - 1u)));
    let y = u32(clamp(floor((rel.y + blocks.y * 0.5) * TEXELS), 0.0, f32(height - 1u)));
    let heat = clamp(cell(y * width + x).x, 0.0, 1.0);
    let stone = posterize(vec3<f32>(0.42, 0.4, 0.4) * (0.75 + 0.35 * hash2(vec2<f32>(f32(x), f32(y)))), 6.0);
    let level = floor(heat * 5.0);
    var flame = vec3<f32>(0.0);
    if level >= 4.0 {
        flame = vec3<f32>(1.0, 0.95, 0.75);
    } else if level >= 3.0 {
        flame = vec3<f32>(1.0, 0.8, 0.2);
    } else if level >= 2.0 {
        flame = vec3<f32>(1.0, 0.45, 0.05);
    } else if level >= 1.0 {
        flame = vec3<f32>(0.7, 0.12, 0.02);
    }
    let burning = level >= 1.0;
    let charred = stone * select(1.0, 0.45, cell(y * width + x).y > 0.5);
    return Surface(select(charred, flame, burning), 1.0, flame * 2.5 * heat);
}
