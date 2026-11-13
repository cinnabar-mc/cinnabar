//! Vendor policy and device budgets independently admit both terrain culling paths.

use bevy::render::renderer::RenderAdapterInfo;

use super::*;

/// Builds a minimal culling app with adapter-owned identity and explicit device limits.
fn cull_app(features: WgpuFeatures, limits: wgpu::Limits) -> (App, wgpu::Instance) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions::enabled(),
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, _) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        required_limits: limits,
        ..Default::default()
    }))
    .unwrap();
    let mut render = SubApp::new();
    render
        .insert_resource(RenderDevice::from(device))
        .insert_resource(RenderAdapter::new(adapter));
    let mut app = App::new();
    app.insert_sub_app(RenderApp, render);
    (app, instance)
}

#[test]
fn minimal_cull_install_uses_adapter_identity_without_an_info_resource() {
    for features in [
        WgpuFeatures::empty(),
        WgpuFeatures::INDIRECT_FIRST_INSTANCE,
        WgpuFeatures::INDIRECT_FIRST_INSTANCE | WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT,
    ] {
        let (mut app, instance) = cull_app(
            features,
            wgpu::Limits {
                max_storage_buffers_per_shader_stage: required_vertex_storage_buffers(),
                ..Default::default()
            },
        );
        install(&mut app);
        let world = app.sub_app(RenderApp).world();
        assert!(!world.contains_resource::<RenderAdapterInfo>());
        assert_eq!(world.resource::<GpuCullSupport>().0, !features.is_empty());
        assert_eq!(
            world.resource::<DirectOcclusionSupport>().0,
            features.is_empty()
        );
        assert_eq!(world.contains_resource::<GpuCull>(), !features.is_empty());
        assert_eq!(
            world.contains_resource::<DirectOcclusion>(),
            features.is_empty()
        );
        assert!(
            instance
                .generate_report()
                .unwrap()
                .hub
                .compute_pipelines
                .num_allocated
                > 0
        );
    }
}

#[test]
fn default_device_limits_keep_terrain_culling_on_the_cpu() {
    assert!(
        wgpu::Limits::default().max_storage_buffers_per_shader_stage
            < required_vertex_storage_buffers()
    );
    let (mut app, instance) = cull_app(WgpuFeatures::INDIRECT_FIRST_INSTANCE, Default::default());
    install(&mut app);
    let world = app.sub_app(RenderApp).world();
    assert!(!world.resource::<GpuCullSupport>().0);
    assert!(!world.resource::<DirectOcclusionSupport>().0);
    assert!(!world.contains_resource::<GpuCull>());
    assert!(!world.contains_resource::<DirectOcclusion>());
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .compute_pipelines
            .num_allocated,
        0
    );
}

#[test]
fn vendor_policy_preserves_supported_desktop_culling_and_blocks_unstable_drivers() {
    let limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: required_vertex_storage_buffers(),
        ..Default::default()
    };
    let downlevel = DownlevelFlags::BASE_VERTEX
        | DownlevelFlags::INDIRECT_EXECUTION
        | DownlevelFlags::COMPUTE_SHADERS;
    for (vendor, unstable) in [
        (0x10de, false),
        (0x1002, false),
        (0x8086, false),
        (ARM_MALI_VENDOR_ID, true),
        (POWERVR_VENDOR_ID, true),
    ] {
        for backend in [Backends::VULKAN, Backends::DX12] {
            for (features, expected) in [
                (WgpuFeatures::empty(), None),
                (
                    WgpuFeatures::INDIRECT_FIRST_INSTANCE,
                    Some(GpuCullSubmission::Fixed),
                ),
                (
                    WgpuFeatures::INDIRECT_FIRST_INSTANCE | WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT,
                    Some(GpuCullSubmission::Count),
                ),
            ] {
                let forced_cpu = cpu_culling_required(vendor, false, &limits);
                assert_eq!(forced_cpu, unstable, "vendor {vendor:#x}");
                let draw_mode = select_chunk_draw_mode(downlevel, features, backend, false);
                assert_eq!(
                    gpu_cull_submission(draw_mode, features, downlevel, forced_cpu, &limits),
                    if unstable { None } else { expected },
                    "vendor {vendor:#x}, backend {backend:?}, features {features:?}"
                );
                assert_eq!(
                    direct_occlusion_supported(draw_mode, downlevel, forced_cpu, &limits),
                    !unstable && features.is_empty(),
                    "vendor {vendor:#x}, backend {backend:?}, features {features:?}"
                );
            }
        }
    }
}

#[test]
fn desktop_vendor_policy_respects_the_cpu_override_and_storage_budget() {
    for vendor in [0x10de, 0x1002, 0x8086] {
        for forced_cpu in [false, true] {
            for storage in [
                required_vertex_storage_buffers() - 1,
                required_vertex_storage_buffers(),
            ] {
                let limits = wgpu::Limits {
                    max_storage_buffers_per_shader_stage: storage,
                    ..Default::default()
                };
                assert_eq!(
                    cpu_culling_required(vendor, forced_cpu, &limits),
                    forced_cpu || storage < required_vertex_storage_buffers()
                );
            }
        }
    }
}

/// A device opened for the last safe graphics level, from an adapter offering everything.
fn safe_graphics_device() -> (WgpuFeatures, wgpu::Limits) {
    let mut settings = bevy::render::settings::WgpuSettings::default();
    crate::device_settings::configure(&mut settings);
    crate::device_settings::configure_safe_graphics(&mut settings, render_api::SafeGraphics::MAX);
    let offered = WgpuFeatures::INDIRECT_FIRST_INSTANCE | WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT;
    let limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: required_vertex_storage_buffers() + 8,
        ..Default::default()
    };
    (
        offered - settings.disabled_features.unwrap(),
        limits.or_worse_values_from(settings.constrained_limits.as_ref().unwrap()),
    )
}

#[test]
fn a_safe_graphics_device_takes_the_constrained_terrain_path_on_every_vendor() {
    let (features, limits) = safe_graphics_device();
    let downlevel = DownlevelFlags::BASE_VERTEX
        | DownlevelFlags::INDIRECT_EXECUTION
        | DownlevelFlags::COMPUTE_SHADERS;
    assert_eq!(
        limits.max_storage_buffers_per_shader_stage,
        minimum_vertex_storage_buffers()
    );
    for vendor in [0x10de, 0x5143, ARM_MALI_VENDOR_ID, POWERVR_VENDOR_ID] {
        let draw_mode = select_chunk_draw_mode(downlevel, features, Backends::VULKAN, false);
        assert_eq!(draw_mode, ChunkDrawMode::Direct);
        let forced_cpu = cpu_culling_required(vendor, false, &limits);
        assert!(forced_cpu, "vendor {vendor:#x}");
        assert_eq!(
            gpu_cull_submission(draw_mode, features, downlevel, forced_cpu, &limits),
            None
        );
        assert!(!direct_occlusion_supported(
            draw_mode, downlevel, forced_cpu, &limits
        ));
        assert!(!crate::chunk::pipeline::units::supported(
            draw_mode, &limits
        ));
    }
}

#[test]
fn a_safe_graphics_device_compiles_no_culling_pipelines() {
    let (features, limits) = safe_graphics_device();
    let (mut app, instance) = cull_app(features, limits);
    install(&mut app);
    let world = app.sub_app(RenderApp).world();
    assert!(!world.resource::<GpuCullSupport>().0);
    assert!(!world.resource::<DirectOcclusionSupport>().0);
    assert!(!world.contains_resource::<GpuCull>());
    assert!(!world.contains_resource::<DirectOcclusion>());
    assert_eq!(
        instance
            .generate_report()
            .unwrap()
            .hub
            .compute_pipelines
            .num_allocated,
        0
    );
}
