// The linear view depth, in blocks, of the opaque scene a world material reads: the nearest
// covered sample's reverse-Z depth turned into distance along the view axis.
@group(0) @binding(0) var source_single: texture_depth_2d;
@group(0) @binding(1) var source_multi: texture_depth_multisampled_2d;
// x: the projection's near plane, which an infinite reverse-Z projection divides by depth.
@group(0) @binding(2) var<uniform> projection: vec4<f32>;

// What the far plane reads as: no opaque surface.
const FAR: f32 = 1.0e9;

// Covers the target with a single triangle.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(corner * 2.0 - vec2(1.0), 0.0, 1.0);
}

fn linear(depth: f32) -> f32 {
    if depth <= 0.0 {
        return FAR;
    }
    return min(projection.x / depth, FAR);
}

@fragment
fn single(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(linear(textureLoad(source_single, vec2<i32>(position.xy), 0)), 0.0, 0.0, 1.0);
}

// The nearest covered sample, as reverse-Z effects read partial edge coverage.
@fragment
fn multisampled(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    var nearest = 0.0;
    for (var sample = 0u; sample < textureNumSamples(source_multi); sample++) {
        nearest = max(nearest, textureLoad(source_multi, vec2<i32>(position.xy), i32(sample)));
    }
    return vec4(linear(nearest), 0.0, 0.0, 1.0);
}
