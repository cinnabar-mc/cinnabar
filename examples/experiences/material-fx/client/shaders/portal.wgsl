// A starfield seen through the block: three star layers sampled from texture 0 at different
// depths shift against each other with the view direction, which reads as parallax, and drift
// slowly while a nebula swirls behind them. The face itself is dark, so only the stars and the
// nebula glow.
// param 0: parallax depth in blocks; params 1-3: nebula colour.
fn shade(input: WorldMaterialInput) -> Surface {
    let depth = param(input, 0u);
    // Look-through offset: the view direction projected onto the face plane, scaled per layer.
    let toward = input.view - input.normal * dot(input.view, input.normal);
    let across = vec2<f32>(dot(toward, vec3<f32>(1.0, 0.0, 0.0)) + dot(toward, vec3<f32>(0.0, 0.0, 1.0)), toward.y);
    let drift = vec2<f32>(fract(input.phase * 64.0), fract(input.phase * 32.0));
    let near = texture(0u, input.uv + across * depth * 0.25 + drift).r;
    let middle = texture(0u, input.uv * 1.7 + across * depth * 0.6 + drift * 0.5).r;
    let far = texture(0u, input.uv * 2.9 + across * depth + drift * 0.25).r;
    let swirl = 0.5 + 0.5 * sin(6.2831853 * (input.uv.x * 1.5 + input.uv.y + fract(input.phase * 256.0)));
    let nebula = vec3<f32>(param(input, 1u), param(input, 2u), param(input, 3u)) * swirl * 0.35;
    let stars = vec3<f32>(near * 1.6 + middle * 0.9 + far * 0.5);
    return Surface(vec3<f32>(0.01, 0.0, 0.03), 1.0, stars + nebula);
}
