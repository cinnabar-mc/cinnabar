// A world material's atlas sprite for shader packs: the guest's surface over the sprite's
// square, written as the albedo, a neutral normal and LabPBR emission in the specular alpha,
// the way a Bedrock block's sprite feeds a pack's lighting. Shader packs light and fog the
// sprite themselves, so the guest sees a white base, full light, no fog and an upward face
// seen from above; world position follows the sprite's UV.
struct WorldSpriteVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn world_sprite_vertex(@builtin(vertex_index) index: u32) -> WorldSpriteVertex {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return WorldSpriteVertex(vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0), vec2<f32>(corner.x, 1.0 - corner.y));
}

struct WorldSpriteTargets {
    @location(0) color: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) specular: vec4<f32>,
}

@fragment
fn world_sprite_fragment(vertex: WorldSpriteVertex) -> WorldSpriteTargets {
    let uv = clamp(vertex.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let surface = world_material_resolve(WorldMaterialInput(
        uv, vec3<f32>(uv.x, 1.0, uv.y), vec4<f32>(1.0), vec4<f32>(1.0), vec4<f32>(0.0),
        vec4<f32>(0.0), world_material_phase(), up, up));
    let colour = min(surface.color + surface.emission, vec3<f32>(1.0));
    // LabPBR 1.3 emission: alpha 0 to 254 of 255.
    let glow = max(max(surface.emission.x, surface.emission.y), surface.emission.z);
    let emissive = clamp(glow, 0.0, 1.0) * (254.0 / 255.0);
    return WorldSpriteTargets(
        vec4<f32>(colour, surface.coverage),
        vec4<f32>(128.0 / 255.0, 128.0 / 255.0, 1.0, 1.0),
        vec4<f32>(0.0, 0.0, 0.0, emissive),
    );
}
