use super::*;

/// Features the client never uses whose driver paths differ from an ordinary device: ray
/// tracing enables buffer device addresses, and the others enable extensions such as mesh
/// shading, 16-bit storage, subgroup size control and descriptor indexing.
const UNUSED_DRIVER_PATHS: Features = Features::EXPERIMENTAL_RAY_QUERY
    .union(Features::EXPERIMENTAL_RAY_HIT_VERTEX_RETURN)
    .union(Features::EXPERIMENTAL_RAY_TRACING_PIPELINES)
    .union(Features::ACCELERATION_STRUCTURE_BINDING_ARRAY)
    .union(Features::EXTENDED_ACCELERATION_STRUCTURE_VERTEX_FORMATS)
    .union(Features::EXPERIMENTAL_MESH_SHADER)
    .union(Features::EXPERIMENTAL_MESH_SHADER_MULTIVIEW)
    .union(Features::EXPERIMENTAL_MESH_SHADER_POINTS)
    .union(Features::EXPERIMENTAL_COOPERATIVE_MATRIX)
    .union(Features::SHADER_F16)
    .union(Features::SHADER_I16)
    .union(Features::SHADER_INT64)
    .union(Features::SUBGROUP)
    .union(Features::MULTIVIEW)
    .union(Features::TEXTURE_BINDING_ARRAY)
    .union(Features::BUFFER_BINDING_ARRAY)
    .union(Features::STORAGE_RESOURCE_BINDING_ARRAY)
    .union(Features::PARTIALLY_BOUND_BINDING_ARRAY)
    .union(Features::CONSERVATIVE_RASTERIZATION)
    .union(Features::PIPELINE_STATISTICS_QUERY)
    .union(Features::TEXTURE_FORMAT_NV12)
    .union(Features::VULKAN_GOOGLE_DISPLAY_TIMING)
    .union(Features::PASSTHROUGH_SHADERS);

#[test]
fn an_adapter_offering_everything_gets_only_client_features() {
    for device_type in [
        wgpu::DeviceType::IntegratedGpu,
        wgpu::DeviceType::DiscreteGpu,
        wgpu::DeviceType::Other,
    ] {
        let features = requested(Features::all(), device_type);
        assert!(
            !features.intersects(UNUSED_DRIVER_PATHS),
            "{device_type:?}: {:?}",
            features & UNUSED_DRIVER_PATHS
        );
        assert_eq!(
            features | Features::MAPPABLE_PRIMARY_BUFFERS,
            CLIENT_FEATURES
        );
    }
}

/// `ExperimentalFeatures::disabled()` makes wgpu reject a device request naming any of these.
#[test]
fn no_client_feature_is_experimental() {
    assert!(!CLIENT_FEATURES.intersects(Features::all_experimental_mask()));
}

#[test]
fn every_offered_client_feature_is_requested() {
    let features = requested(Features::all(), wgpu::DeviceType::IntegratedGpu);
    assert_eq!(features, CLIENT_FEATURES);
    assert!(features.contains(RENDERER_FEATURES));
    assert!(features.contains(SHADER_PACK_FEATURES));
}

#[test]
fn features_the_adapter_lacks_are_never_requested() {
    let offered = Features::INDIRECT_FIRST_INSTANCE
        | Features::TIMESTAMP_QUERY
        | Features::EXPERIMENTAL_RAY_QUERY
        | Features::SHADER_F16;
    assert_eq!(
        requested(offered, wgpu::DeviceType::IntegratedGpu),
        Features::INDIRECT_FIRST_INSTANCE | Features::TIMESTAMP_QUERY
    );
    assert_eq!(
        requested(Features::empty(), wgpu::DeviceType::IntegratedGpu),
        Features::empty()
    );
}

#[test]
fn discrete_gpus_do_not_map_device_local_buffers() {
    let offered = Features::MAPPABLE_PRIMARY_BUFFERS | Features::PIPELINE_CACHE;
    assert_eq!(
        requested(offered, wgpu::DeviceType::DiscreteGpu),
        Features::PIPELINE_CACHE
    );
    assert_eq!(requested(offered, wgpu::DeviceType::IntegratedGpu), offered);
}
