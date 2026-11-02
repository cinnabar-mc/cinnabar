struct Projection { clip_from_hand: mat4x4<f32> }
@group(0) @binding(0) var<uniform> projection: Projection;
@group(0) @binding(1) var skin: texture_2d<f32>;
@group(0) @binding(2) var nearest: sampler;
struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn hand_vertex(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>) -> VertexOut {
    var output: VertexOut;
    output.position = projection.clip_from_hand * vec4<f32>(position, 1.0);
    output.uv = uv;
    return output;
}
@fragment fn hand_fragment(input: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(skin, nearest, input.uv);
    if color.a == 0.0 { discard; }
    // Without blending this replaces the scene; a separately composed hand keeps that coverage.
    return vec4<f32>(color.rgb, 1.0);
}
