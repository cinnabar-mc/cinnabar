// Heat haze: the air above the lava lamp shimmers. The block is invisible but for bending the
// world behind it: rising pixel waves shift what the screen shows by a few pixels, strongest
// low down, fading to nothing at the top. Needs the scene; without it the block draws nothing.
// param 0: heat (0 to 1).

fn shade(input: WorldMaterialInput) -> Surface {
    let local = local_of(input);
    let time = seconds(input);
    let texel = floor(face_uv(local, input.normal) * TEXELS);
    let rise = local.y;
    let strength = clamp(param(input, 0u), 0.0, 1.0) * (1.0 - rise);
    let wave = sin(texel.y * 0.9 - time * 6.0 + texel.x * 0.3) + sin(texel.x * 1.7 + time * 3.1);
    let shift = vec2<f32>(round(wave * 2.0 * strength), round(sin(time * 5.0 + texel.x) * strength));
    let behind = scene_color(shift);
    let coverage = select(0.0, 1.0, strength > 0.01);
    return Surface(vec3<f32>(0.0), coverage, behind);
}
