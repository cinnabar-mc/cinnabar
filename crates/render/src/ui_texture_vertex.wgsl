// Converts texture vertices before interpolation and retains resident atlas placement.
fn ui_texture_vertex(uv: vec2<f32>, atlas_offset: vec2<f32>, style_flags: u32, dimensions: vec2<f32>) -> vec4<f32> {
    if (style_flags & STYLE_UNORM16_UV) == 0u {
        return vec4<f32>(uv, atlas_offset);
    }
    let packed = clamp(floor((uv + atlas_offset) / dimensions * 65536.0), vec2<f32>(0.0), vec2<f32>(65535.0));
    return vec4<f32>(packed / 65535.0, 0.0, 0.0);
}
