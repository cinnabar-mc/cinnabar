// Dissolve with a burning edge. Texture 0 is tileable value noise. Texels below the cut are
// gone, and a band just above it burns in the edge colour. The cut is parameter 5 when it is
// between 0 and 1, which the client part animates on command; otherwise it sweeps through the
// noise and back every four seconds (1024 cycles per 4096 s phase wrap).
// param 0: edge width in noise units; params 1-3: edge colour; param 4: edge emission;
// param 5: the cut, or below 0 to sweep.
fn shade(input: WorldMaterialInput) -> Surface {
    let noise = texture(0u, input.uv * 2.0).r;
    let sweep = 0.5 - 0.5 * cos(6.2831853 * fract(input.phase * 1024.0));
    let held = param(input, 5u);
    let cut = select(mix(-0.05, 1.05, sweep), held, held >= 0.0);
    let above = noise - cut;
    let width = max(param(input, 0u), 0.001);
    let edge = 1.0 - smoothstep(0.0, width, above);
    let edge_colour = vec3<f32>(param(input, 1u), param(input, 2u), param(input, 3u));
    let coverage = select(0.0, input.base.a, above >= 0.0);
    let colour = mix(input.base.rgb, edge_colour, edge);
    return Surface(colour, coverage, edge_colour * edge * param(input, 4u));
}
