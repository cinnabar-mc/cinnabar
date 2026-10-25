// Render-resolution motion vectors for temporal upscalers: for every pixel, the offset in render
// pixels from its current position to where the same surface was in the previous frame,
// without projection jitter. Static geometry and the sky reproject exactly through the camera.

@group(0) @binding(0) var depth: texture_depth_2d;

// Mirrors `MotionUniform` in temporal.rs.
struct Motion {
    previous_clip_from_clip: mat4x4<f32>,
    // Width and height in render pixels; zw unused.
    viewport: vec4<f32>,
}
@group(0) @binding(1) var<uniform> motion: Motion;

/// Covers the target with one triangle.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn motion_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    let size = motion.viewport.xy;
    let uv = position.xy / size;
    // Reverse-Z: the sky clears to 0, a direction at infinity that camera translation leaves.
    let z = textureLoad(depth, vec2<i32>(position.xy), 0);
    let previous = motion.previous_clip_from_clip
        * vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), z, 1.0);
    if previous.w <= 0.00001 {
        return vec2<f32>(0.0);
    }
    let previous_uv = previous.xy / previous.w * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return (previous_uv - uv) * size;
}


// Hands are drawn with their own depth and move with the camera, so where a hand covers the
// world its screen-space motion is zero. Bound with the hand's depth in place of the world's.
@fragment
fn hand_mask_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    if textureLoad(depth, vec2<i32>(position.xy), 0) <= 0.0 {
        discard;
    }
    return vec2<f32>(0.0);
}