// Dissolve with a burning edge. Texture 0 is tileable value noise, read once per skin texel so
// the wraith comes apart pixel by pixel. Texels below the cut are gone, the band just above it
// burns in two posterized steps, and embers flicker on the burning texels. The cut is parameter
// 5 when it is between 0 and 1, which the client part animates on command; otherwise it sweeps
// through the noise and back every four seconds.
// param 0: edge width in noise units; params 1-3: edge colour; param 4: edge emission;
// param 5: the cut, or below 0 to sweep.

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let texel = floor(input.uv * 64.0);
    let noise = texture(0u, (texel + 0.5) / 32.0).r;
    let sweep = 0.5 - 0.5 * cos(TAU * fract(time / 4.0));
    let held = param(input, 5u);
    let cut = select(mix(-0.05, 1.05, sweep), held, held >= 0.0);
    let above = noise - cut;
    let width = max(param(input, 0u), 0.001);
    let edge = floor((1.0 - clamp(above / width, 0.0, 1.0)) * 2.0 + 0.5) / 2.0;
    let edge_colour = vec3<f32>(param(input, 1u), param(input, 2u), param(input, 3u));
    let coverage = select(0.0, input.base.a, above >= 0.0);
    let ember = select(0.0, 1.0, edge > 0.0 && hash3(vec3<f32>(texel, floor(time * 10.0))) > 0.7);
    let colour = mix(input.base.rgb, edge_colour, edge);
    return Surface(colour, coverage, edge_colour * (edge + ember * 0.8) * param(input, 4u));
}
