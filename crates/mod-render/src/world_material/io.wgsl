struct WorldMaterialInput {
    // Texture coordinates of the Bedrock surface.
    uv: vec2<f32>,
    world_position: vec3<f32>,
    // Bedrock material colour before lighting: texture times tint; alpha is its coverage.
    base: vec4<f32>,
    // The Bedrock light colour the host multiplies the returned colour by, and in alpha the
    // ambient occlusion and face shade it already includes (1 where there is none).
    light: vec4<f32>,
    // Fog colour and the fog amount the host applies after lighting.
    fog: vec4<f32>,
    // Actor overlay colour such as the hurt flash; zero for blocks.
    overlay: vec4<f32>,
    // The session's animation phase in [0, 1).
    phase: f32,
}
struct Surface { color: vec3<f32>, coverage: f32, emission: vec3<f32> }
// The Bedrock surface unchanged, which the host lights as an ordinary material.
fn surface(input: WorldMaterialInput) -> Surface {
    return Surface(input.base.rgb, input.base.a, vec3<f32>(0.0));
}
// Comparisons reject NaN and negative values before bounding guest arithmetic.
fn world_material_bounded(value: f32, upper: f32) -> f32 {
    if value >= 0.0 { return min(value, upper); }
    return 0.0;
}
// Bounds the guest surface: colour and coverage to [0, 1], emission to the host maximum.
fn world_material_resolve(input: WorldMaterialInput) -> Surface {
    let surface = shade(input);
    return Surface(
        vec3<f32>(world_material_bounded(surface.color.x, 1.0), world_material_bounded(surface.color.y, 1.0), world_material_bounded(surface.color.z, 1.0)),
        world_material_bounded(surface.coverage, 1.0),
        vec3<f32>(world_material_bounded(surface.emission.x, MAX_EMISSION), world_material_bounded(surface.emission.y, MAX_EMISSION), world_material_bounded(surface.emission.z, MAX_EMISSION)),
    );
}
