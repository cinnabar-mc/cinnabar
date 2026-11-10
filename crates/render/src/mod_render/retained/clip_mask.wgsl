@group(1) @binding(0) var world_depth: texture_depth_2d;

// The world gates the mask without populating its contents' private depth.
@fragment
fn clip_mask_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let depth = textureLoad(world_depth, vec2<i32>(position.xy), 0);
    if position.z < depth { discard; }
    return vec4(0.0);
}
