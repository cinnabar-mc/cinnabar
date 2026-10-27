// A glowing waypoint beam: a bright core that fades to its edges across the beam's width
// (uv.x) and towards its top (uv.y), with pulses of light travelling up it. The beam's colour
// is its vertex colour times the instance tint.
// param 0: pulses along the beam; param 1: pulse speed in cycles per 4 s; param 2: emission.
fn shade(input: MaterialInput) -> Surface {
    let across = 1.0 - abs(input.uv.x * 2.0 - 1.0);
    let core = across * across;
    let height = clamp(input.uv.y, 0.0, 1.0);
    let travel = fract(input.uv.y * param(input, 0u) - fract(phase() * 1024.0) * param(input, 1u));
    let pulse = smoothstep(0.75, 1.0, travel);
    let fade = 1.0 - height * height;
    let glow = input.color.rgb * (core * (0.5 + 1.5 * pulse)) * param(input, 2u);
    return Surface(input.color.rgb * core, core * fade * input.color.a, glow * fade);
}
