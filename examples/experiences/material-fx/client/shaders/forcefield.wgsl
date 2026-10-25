// Force field: a shimmering pixel hex shield. Where it meets the ground or anything else solid
// behind it, a soft bright seam glows, read from how close the opaque scene is behind each
// fragment; pulses from the server ripple across it. Needs the scene for the seam.
// params 0-2: the field's centre; param 3: a pulse's start in session seconds.

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let texel = floor(input.world_position * TEXELS);
    // A hex lattice on the 16-texel grid: rows offset by half a cell.
    let row = floor(texel.y / 3.0);
    let column = floor((texel.x + texel.z + select(0.0, 2.0, row % 2.0 == 0.0)) / 4.0);
    let cell = vec2<f32>(row, column);
    let edge = (u32(texel.y) % 3u) == 0u || (u32(texel.x + texel.z + select(0.0, 2.0, row % 2.0 == 0.0)) % 4u) == 0u;
    let shimmer = 0.5 + 0.5 * sin(time * 2.0 + hash2(cell) * 6.0);
    var glow = vec3<f32>(0.2, 0.55, 1.0) * select(0.08, 0.35 * shimmer, edge);
    // Soft seam where the opaque scene is close behind.
    let gap = scene_depth(vec2<f32>(0.0)) - fragment_depth();
    let seam = clamp(1.0 - gap / 0.6, 0.0, 1.0);
    glow = glow + vec3<f32>(0.5, 0.85, 1.0) * floor(seam * seam * 4.0) / 4.0 * 1.8;
    // A pulse ring travelling out from the centre.
    let centre = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let age = since(input, param(input, 3u));
    let reach = distance(snap3(input.world_position), centre);
    let ring = select(0.0, 1.0, abs(reach - age * 4.0) < 0.2 && age < 2.0);
    glow = glow + vec3<f32>(0.7, 0.95, 1.0) * ring * (1.0 - age / 2.0) * 2.0;
    return Surface(vec3<f32>(0.0), 1.0, glow);
}
