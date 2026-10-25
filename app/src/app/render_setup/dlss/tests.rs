use super::*;

/// A registration call that returned without an NGX error.
const REGISTERED: Result<(), dlss_wgpu::DlssError> = Ok(());

#[test]
fn registration_enables_dlss_only_when_super_resolution_extensions_were_added() {
    let supported = FeatureSupport::default();
    assert_eq!(unsupported(REGISTERED, &supported), None);

    let missing = FeatureSupport {
        super_resolution_supported: false,
        ..FeatureSupport::default()
    };
    assert!(unsupported(REGISTERED, &missing).is_some());

    let failed = Err(dlss_wgpu::DlssError::FeatureNotSupported);
    assert!(unsupported(failed, &supported).is_some());
}

#[test]
fn ray_reconstruction_support_does_not_decide_dlss() {
    let support = FeatureSupport {
        ray_reconstruction_supported: false,
        ..FeatureSupport::default()
    };
    assert_eq!(unsupported(REGISTERED, &support), None);
}

/// A device request whose features and limits are exactly those given.
fn descriptor(features: wgpu::Features, limits: wgpu::Limits) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        required_features: features,
        required_limits: limits,
        ..Default::default()
    }
}

#[test]
fn hal_device_is_opened_only_within_the_adapter_capabilities() {
    let adapter_limits = wgpu::Limits::default();
    let features = wgpu::Features::DEPTH_CLIP_CONTROL;
    let within = descriptor(features, adapter_limits.clone());
    assert!(adapter_satisfies(&within, features, &adapter_limits, false));

    let extra_feature = descriptor(
        features | wgpu::Features::TIMESTAMP_QUERY,
        adapter_limits.clone(),
    );
    assert!(!adapter_satisfies(
        &extra_feature,
        features,
        &adapter_limits,
        false
    ));

    let raised_limit = descriptor(
        features,
        wgpu::Limits {
            max_texture_dimension_2d: adapter_limits.max_texture_dimension_2d + 1,
            ..adapter_limits.clone()
        },
    );
    assert!(!adapter_satisfies(
        &raised_limit,
        features,
        &adapter_limits,
        false
    ));
}

#[test]
fn experimental_features_need_experimental_opt_in() {
    let limits = wgpu::Limits::default();
    let experimental = wgpu::Features::all_experimental_mask()
        .iter()
        .next()
        .expect("wgpu has experimental features");
    let request = descriptor(experimental, limits.clone());
    assert!(!adapter_satisfies(&request, experimental, &limits, false));
    assert!(adapter_satisfies(&request, experimental, &limits, true));
}
