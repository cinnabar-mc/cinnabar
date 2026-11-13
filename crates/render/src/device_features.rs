//! The wgpu features the client requests when it opens its device.
//!
//! The device gets only the features listed here, and only those the adapter offers. Every one
//! is optional: each consumer checks `device.features()` and falls back without it. A feature
//! left out stays off even when the driver exposes it, because enabling one can change the
//! driver's work for every allocation or pipeline. Ray query, for example, also enables buffer
//! device addresses, which puts a device address on every memory allocation.
//!
//! The architecture gate (`[device_features]` in its policy) keeps this file and its uses in
//! step: production code may name no feature constant missing from this file, and every
//! [`RENDERER_FEATURES`] entry must still be named by production code.

use wgpu::Features;

/// Features that renderer code checks by name before using them.
pub const RENDERER_FEATURES: Features = Features::INDIRECT_FIRST_INSTANCE
    .union(Features::MULTI_DRAW_INDIRECT_COUNT)
    .union(Features::TIMESTAMP_QUERY)
    .union(Features::TIMESTAMP_QUERY_INSIDE_PASSES)
    .union(Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)
    .union(Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    .union(Features::SHADER_EARLY_DEPTH_TEST)
    .union(Features::MAPPABLE_PRIMARY_BUFFERS)
    .union(Features::TEXTURE_ATOMIC)
    .union(Features::PIPELINE_CACHE)
    .union(Features::FLOAT32_FILTERABLE)
    .union(Features::RG11B10UFLOAT_RENDERABLE)
    .union(Features::TEXTURE_FORMAT_16BIT_NORM);

/// Features that installed shader packs need without any code naming them. wgpu validates
/// each translated pack program against the device's features, and these cover what the pack
/// compiler passes through: `gl_PrimitiveID`, `double`, `gl_ClipDistance`, `gl_DrawID`, and
/// images or buffers that a vertex stage writes. A pack needing a feature outside this set
/// fails its own load; it does not affect the device.
pub const SHADER_PACK_FEATURES: Features = Features::PRIMITIVE_INDEX
    .union(Features::SHADER_F64)
    .union(Features::CLIP_DISTANCES)
    .union(Features::SHADER_DRAW_INDEX)
    .union(Features::VERTEX_WRITABLE_STORAGE);

/// Every feature the client may request.
pub const CLIENT_FEATURES: Features = RENDERER_FEATURES.union(SHADER_PACK_FEATURES);

/// The client features `available` offers. Discrete GPUs leave out `MAPPABLE_PRIMARY_BUFFERS`,
/// whose mapped device-local buffers would read across the PCIe bus.
pub fn requested(available: Features, device_type: wgpu::DeviceType) -> Features {
    let mut features = available & CLIENT_FEATURES;
    if device_type == wgpu::DeviceType::DiscreteGpu {
        features.remove(Features::MAPPABLE_PRIMARY_BUFFERS);
    }
    features
}

#[cfg(test)]
mod tests;
