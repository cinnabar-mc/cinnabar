use super::*;

#[test]
fn raster_occlusion_requires_vulkan_and_both_depth_and_storage_capabilities() {
    use wgpu::{Backend, DownlevelFlags as D, Features as F};
    let flags = D::VERTEX_STORAGE | D::FRAGMENT_WRITABLE_STORAGE;
    let feature = F::SHADER_EARLY_DEPTH_TEST;
    assert!(supported(Backend::Vulkan, feature, flags));
    for backend in [
        Backend::Metal,
        Backend::Dx12,
        Backend::Gl,
        Backend::BrowserWebGpu,
    ] {
        assert!(!supported(backend, feature, flags));
    }
    assert!(!supported(Backend::Vulkan, F::empty(), flags));
    for removed in [D::VERTEX_STORAGE, D::FRAGMENT_WRITABLE_STORAGE] {
        assert!(!supported(Backend::Vulkan, feature, flags - removed));
    }
}

#[test]
fn raster_shader_validates_with_forced_early_depth_and_shared_record_layout() {
    let source = shader_source(super::super::model::CULL_WORKGROUP);
    let module = naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}
