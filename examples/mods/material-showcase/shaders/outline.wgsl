// An animated outline: dashes march along the line (its cumulative distance) and the colour
// cycles through the hue wheel along the outline and over time, glowing.
// param 0: dashes per block; param 1: march speed in cycles per 4 s; param 2: emission.
fn shade(input: MaterialInput) -> Surface {
    let march = fract(input.distance * param(input, 0u) - fract(phase() * 1024.0) * param(input, 1u));
    let dash = select(0.0, 1.0, march < 0.6);
    let hue = 6.2831853 * (input.distance * 0.25 + fract(phase() * 256.0));
    let colour = 0.5 + 0.5 * cos(vec3<f32>(hue, hue - 2.0943951, hue + 2.0943951));
    return Surface(colour, dash * input.color.a, colour * dash * param(input, 2u));
}
