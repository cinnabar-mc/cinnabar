// Hologram: an additive projection whose rim, where the surface turns away from the viewer,
// glows brightest. Scanlines one texel tall drift upwards, the image flickers, and now and then
// a glitch slips a band of rows sideways. Everything sits on the skin's 64-texel grid and the
// world's 16-texel rows.
// params 0-2: hologram colour; param 3: scanline density per block.

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let colour = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let facing = clamp(abs(dot(input.normal, input.view)), 0.0, 1.0);
    let rim = floor(pow(1.0 - facing, 2.0) * 4.0 + 0.5) / 4.0;
    let row = floor(input.world_position.y * 16.0);
    let density = max(param(input, 3u), 1.0);
    let scan = select(0.35, 1.0, fract((row - floor(time * 12.0)) / (16.0 / density)) < 0.5);
    let glitch_band = hash2(vec2<f32>(floor(row / 3.0), floor(time * 5.0))) > 0.93;
    let texel = floor(input.uv * 64.0) + select(vec2<f32>(0.0), vec2<f32>(3.0, 0.0), glitch_band);
    let flicker = select(1.0, 0.55, hash2(vec2<f32>(floor(time * 15.0), 1.0)) > 0.9);
    let luminance = dot(input.base.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let grain = 0.85 + 0.15 * hash2(texel + floor(time * 8.0));
    let body = colour * (0.2 + 0.5 * floor(luminance * 4.0 + 0.5) / 4.0) * scan * grain;
    return Surface(body * flicker * 0.4, 1.0, (body * 1.4 + colour * rim * 2.5) * flicker);
}
