// Status tint and damage flash, driven by parameters the client part updates at run time
// without recompiling: the status colour pulses over the body and glows faintly, and a
// damage flash washes the body white and fades as the client part lowers it.
// params 0-2: status colour; param 3: status strength; param 4: flash intensity.
fn shade(input: WorldMaterialInput) -> Surface {
    let status = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let strength = clamp(param(input, 3u), 0.0, 1.0);
    let flash = clamp(param(input, 4u), 0.0, 1.0);
    let pulse = 0.65 + 0.35 * sin(6.2831853 * fract(input.phase * 2048.0));
    let tinted = mix(input.base.rgb, input.base.rgb * status + status * 0.2, strength * pulse);
    let colour = mix(tinted, vec3<f32>(1.0), flash);
    return Surface(colour, input.base.a, status * strength * pulse * 0.6 + vec3<f32>(flash * 3.0));
}
