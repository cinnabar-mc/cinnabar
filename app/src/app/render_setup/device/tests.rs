use super::*;

#[test]
fn release_indirect_validation_follows_the_selected_backend() {
    let configured = wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL
        | wgpu::InstanceFlags::DEBUG
        | wgpu::InstanceFlags::GPU_BASED_VALIDATION;
    for backend in [Backend::Metal, Backend::Vulkan, Backend::Gl] {
        assert_eq!(
            backend_instance_flags(backend, configured, false),
            configured - wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL
        );
        assert_eq!(
            backend_instance_flags(backend, configured, true),
            configured
        );
    }
    assert_eq!(
        backend_instance_flags(Backend::Dx12, configured, false),
        configured
    );
}

#[test]
fn vulkan_minimum_storage_limit_is_accepted() {
    assert!(meets_requirements(capabilities(REQUIRED_DOWNLEVEL_FLAGS, 4)).is_ok());
}

/// Supplies native-independent capabilities to the production candidate filter.
fn capabilities(flags: wgpu::DownlevelFlags, storage_buffers: u32) -> AdapterCapabilities {
    AdapterCapabilities {
        flags,
        limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: storage_buffers,
            ..Default::default()
        },
    }
}

#[test]
fn adapters_must_support_every_renderer_capability_and_storage_limit() {
    let required = render::minimum_vertex_storage_buffers();
    assert!(meets_requirements(capabilities(REQUIRED_DOWNLEVEL_FLAGS, required)).is_ok());
    for flag in REQUIRED_DOWNLEVEL_FLAGS.iter() {
        assert!(
            meets_requirements(capabilities(REQUIRED_DOWNLEVEL_FLAGS - flag, required)).is_err()
        );
    }
    assert!(meets_requirements(capabilities(REQUIRED_DOWNLEVEL_FLAGS, required - 1)).is_err());
}

#[test]
fn adapters_without_alternate_texture_views_are_not_candidates() {
    let view_formats = wgpu::DownlevelFlags::VIEW_FORMATS;
    assert!(REQUIRED_DOWNLEVEL_FLAGS.contains(view_formats));
    // Keep every other capability, isolating the missing GL/GLES texture-view support.
    let adapter = capabilities(
        wgpu::DownlevelFlags::all() - view_formats,
        render::required_vertex_storage_buffers(),
    );
    let error = meets_requirements(adapter).unwrap_err();
    assert!(error.to_string().contains("VIEW_FORMATS"));
}

#[test]
fn configured_storage_limits_cover_the_renderer_without_lowering_existing_limits() {
    let required = render::required_vertex_storage_buffers();
    for initial in [0, required, required + 1] {
        let mut settings = WgpuSettings::default();
        settings.limits.max_storage_buffers_per_shader_stage = initial;
        render::device_settings::configure(&mut settings);
        assert_eq!(
            settings.limits.max_storage_buffers_per_shader_stage,
            initial.max(required)
        );
    }
}

/// An adapter that offers every wgpu feature, as the newest mobile GPUs nearly do.
fn offering_everything(device_type: wgpu::DeviceType) -> AdapterOffer {
    AdapterOffer {
        features: wgpu::Features::all(),
        limits: wgpu::Limits::default(),
        device_type,
    }
}

#[test]
fn the_device_requests_only_the_renderer_feature_list_from_an_adapter_offering_everything() {
    let settings = WgpuSettings::default();
    for device_type in [
        wgpu::DeviceType::IntegratedGpu,
        wgpu::DeviceType::DiscreteGpu,
    ] {
        let (features, _) = device_requirements(&offering_everything(device_type), &settings);
        assert_eq!(
            features,
            render::device_features::requested(wgpu::Features::all(), device_type)
                | settings.features
        );
        // Ray tracing would enable acceleration structures and buffer device addresses.
        let unused = wgpu::Features::all_experimental_mask()
            | wgpu::Features::ACCELERATION_STRUCTURE_BINDING_ARRAY
            | wgpu::Features::EXTENDED_ACCELERATION_STRUCTURE_VERTEX_FORMATS
            | wgpu::Features::SHADER_F16
            | wgpu::Features::SHADER_INT64
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SUBGROUP;
        assert!(!features.intersects(unused), "{}", features & unused);
        assert!(features.contains(
            render::device_features::RENDERER_FEATURES - wgpu::Features::MAPPABLE_PRIMARY_BUFFERS
        ));
    }
}

#[test]
fn settings_can_still_disable_and_require_features() {
    let offer = offering_everything(wgpu::DeviceType::IntegratedGpu);
    let settings = WgpuSettings {
        disabled_features: Some(wgpu::Features::TIMESTAMP_QUERY),
        features: wgpu::Features::DEPTH_CLIP_CONTROL,
        ..Default::default()
    };
    let (features, _) = device_requirements(&offer, &settings);
    assert!(!features.contains(wgpu::Features::TIMESTAMP_QUERY));
    assert!(features.contains(wgpu::Features::DEPTH_CLIP_CONTROL));
    let portable = WgpuSettings {
        priority: WgpuSettingsPriority::WebGPU,
        ..Default::default()
    };
    assert_eq!(device_requirements(&offer, &portable).0, portable.features);
}

/// Renderer and shader-pack allocations are sized from the device's limits.
#[test]
fn the_device_keeps_the_adapter_limits() {
    let mut offer = offering_everything(wgpu::DeviceType::IntegratedGpu);
    offer.limits.max_buffer_size = 1 << 32;
    offer.limits.max_storage_buffer_binding_size = 1 << 31;
    offer.limits.max_texture_array_layers = 2048;
    let (_, limits) = device_requirements(&offer, &WgpuSettings::default());
    assert_eq!(limits, offer.limits);
}

#[test]
fn the_device_request_record_names_the_adapter_driver_and_features() {
    let info = wgpu::AdapterInfo {
        name: "Adreno (TM) 750".into(),
        vendor: 0x5143,
        device: 0x43051401,
        driver: "Qualcomm Technologies Inc. Adreno Vulkan Driver".into(),
        driver_info: "Driver Build: 0762.10".into(),
        ..wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Vulkan)
    };
    let features = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::PIPELINE_CACHE;
    let record = device_request_summary(&info, features);
    for part in [
        "\"Adreno (TM) 750\"",
        "vendor 0x5143",
        "device 0x43051401",
        "\"Qualcomm Technologies Inc. Adreno Vulkan Driver\"",
        "\"Driver Build: 0762.10\"",
        "backend Vulkan",
        "TIMESTAMP_QUERY",
        "PIPELINE_CACHE",
    ] {
        assert!(record.contains(part), "{record}");
    }
    assert!(!record.contains('\n'));
}

/// Window targets use the adapter's full dimensions without requesting unsupported limits.
#[test]
fn window_texture_limits_match_the_adapter_even_for_oversized_requests() {
    let adapter = wgpu::Limits {
        max_texture_dimension_1d: 16384,
        max_texture_dimension_2d: 16384,
        ..wgpu::Limits::default()
    };
    for initial in [4096, 16384, u32::MAX] {
        let mut limits = wgpu::Limits {
            max_texture_dimension_1d: initial,
            max_texture_dimension_2d: initial,
            ..Default::default()
        };
        use_adapter_texture_limits(&mut limits, &adapter);
        assert_eq!(
            limits.max_texture_dimension_1d,
            adapter.max_texture_dimension_1d
        );
        assert_eq!(
            limits.max_texture_dimension_2d,
            adapter.max_texture_dimension_2d
        );
    }
}

/// wgpu drops the DX12 backend when its DXC cannot load; only that case retries with FXC.
#[cfg(windows)]
#[test]
fn dx12_retries_with_fxc_only_when_its_dxc_cannot_load() {
    let settings = WgpuSettings::default();
    let fxc = wgpu::Dx12Compiler::Fxc;
    let with_fxc = instance(Backend::Dx12, &settings, fxc.clone(), None);
    if !has_dx12(&with_fxc) {
        eprintln!("missing fixture: a DX12 runtime");
        return;
    }
    assert!(!dxc_failed(Backend::Dx12, &fxc, &with_fxc));
    let missing = wgpu::Dx12Compiler::DynamicDxc {
        dxc_path: std::env::temp_dir()
            .join("cinnabar-missing-dxc")
            .join("dxcompiler.dll")
            .to_string_lossy()
            .into_owned(),
    };
    let without_dxc = instance(Backend::Dx12, &settings, missing.clone(), None);
    assert!(dxc_failed(Backend::Dx12, &missing, &without_dxc));
    assert!(!dxc_failed(Backend::Vulkan, &missing, &without_dxc));
}
