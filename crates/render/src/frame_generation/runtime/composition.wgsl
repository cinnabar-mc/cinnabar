// The shared UI composite source supplies colour conversion, gamma blending and the vertex entry.
@group(0) @binding(3) var label_single: texture_2d<f32>;
@group(0) @binding(4) var overlay_sampler: sampler;

/// Applies the current, unwarped premultiplied hand after sharp world labels.
fn finish_overlay(position: vec4<f32>, labelled: vec4<f32>) -> vec4<f32> {
    let uv = position.xy / vec2<f32>(textureDimensions(scene));
    let hand = textureSampleLevel(ui_layer, overlay_sampler, uv, 0.0);
    return vec4(hand.rgb + labelled.rgb * (1.0 - hand.a), 1.0);
}

/// Preserves each name sample's encoded blending before resolving coverage to native pixels.
@fragment
fn overlay_multi(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let size = vec2<f32>(textureDimensions(scene));
    let label_pixel = vec2<i32>(position.xy * vec2<f32>(textureDimensions(nametag_samples)) / size);
    let under = textureLoad(scene, pixel, 0);
    var labelled = vec4(0.0);
    let count = textureNumSamples(nametag_samples);
    for (var i = 0u; i < count; i++) {
        labelled += composite_sample(textureLoad(nametag_samples, label_pixel, i32(i)), under, true);
    }
    return finish_overlay(position, labelled / f32(count));
}

/// Composes single-sampled world labels and the hand without temporal resampling.
@fragment
fn overlay_single(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let label_pixel = vec2<i32>(position.xy * vec2<f32>(textureDimensions(label_single)) / vec2<f32>(textureDimensions(scene)));
    return finish_overlay(position, composite_sample(textureLoad(label_single, label_pixel, 0), textureLoad(scene, pixel, 0), true));
}

/// Copies an already-linear sampled real image to a matching output attachment.
@fragment
fn copy_linear(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(textureLoad(scene, vec2<i32>(position.xy), 0).rgb, 1.0);
}

/// Decodes a generated SDR image before writing its native sRGB render target.
@fragment
fn copy_generated(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(srgb_to_linear(textureLoad(scene, vec2<i32>(position.xy), 0).rgb), 1.0);
}
