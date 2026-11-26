//! wgpu's strict WebGPU compliance mode, which rejects some native-only render pass options.

/// Whether wgpu's strict WebGPU compliance is on, under which a render pass may not begin with
/// `LoadOp::DontCare`. Cinnabar sets that flag only through wgpu's environment override, which
/// `device_settings::configure` and the client's instance setup both reapply, so the same read
/// answers here.
pub(crate) fn enabled() -> bool {
    static STRICT: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
        wgpu::InstanceFlags::empty()
            .with_env()
            .contains(wgpu::InstanceFlags::STRICT_WEBGPU_COMPLIANCE)
    });
    *STRICT
}
