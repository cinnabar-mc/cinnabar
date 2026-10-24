// Hologram: an additive projection whose rim, where the surface turns away from the viewer,
// glows brightest; horizontal scanlines drift upwards and the whole image flickers.
// params 0-2: hologram colour; param 3: scanline density per block.
fn shade(input: WorldMaterialInput) -> Surface {
    let colour = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let facing = clamp(abs(dot(input.normal, input.view)), 0.0, 1.0);
    let rim = pow(1.0 - facing, 2.0);
    let drift = fract(input.phase * 512.0);
    let scan = 0.5 + 0.5 * sin(6.2831853 * (input.world_position.y * param(input, 3u) - drift * 4.0));
    let flicker = 0.85 + 0.15 * sin(6.2831853 * fract(input.phase * 61440.0));
    let luminance = dot(input.base.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let body = colour * (0.15 + 0.35 * luminance) * (0.6 + 0.4 * scan);
    return Surface(body * flicker, 1.0, colour * (0.3 * scan + 2.5 * rim) * flicker);
}
