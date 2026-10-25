//! Vulkan instance and device creation with the extensions NVIDIA NGX needs for DLSS.
//!
//! Every failure returns `None` so the caller builds its usual instance or device; DLSS never
//! decides whether a renderer starts.

use dlss_wgpu::FeatureSupport;
use wgpu::hal::{api::Vulkan, vulkan};
use wgpu::rwh::HasDisplayHandle as _;

use super::device::log;

/// Creates a Vulkan-only instance carrying DLSS Super Resolution's instance extensions, or `None`
/// when they are unavailable. wgpu keeps the core-level instance flags at their build defaults
/// for instances created from hal, so the usual instance is preferred whenever DLSS can't use
/// this one.
pub(super) fn instance(descriptor: &wgpu::InstanceDescriptor) -> Option<wgpu::Instance> {
    let mut support = FeatureSupport::default();
    let mut registered = Ok(());
    let hal_descriptor = wgpu::hal::InstanceDescriptor {
        name: "wgpu",
        flags: descriptor.flags,
        memory_budget_thresholds: descriptor.memory_budget_thresholds,
        backend_options: descriptor.backend_options.clone(),
        telemetry: None,
        display: descriptor
            .display
            .as_ref()
            .and_then(|display| display.display_handle().ok()),
    };
    // SAFETY: The callback only appends instance extensions NGX found in this Vulkan loader.
    let created = unsafe {
        vulkan::Instance::init_with_callback(
            &hal_descriptor,
            Some(Box::new(|mut args| {
                registered = dlss_wgpu::register_instance_extensions(
                    render::DLSS_PROJECT_ID,
                    &mut args,
                    &mut support,
                );
            })),
        )
    };
    let created = match created {
        Ok(created) => created,
        Err(error) => {
            log(format_args!(
                "Renderer DLSS Vulkan instance failed; using the default instance: {error}"
            ));
            return None;
        }
    };
    if let Some(reason) = unsupported(registered, &support) {
        log(format_args!("Renderer DLSS unavailable: {reason}"));
        return None;
    }
    // SAFETY: `created` is a live Vulkan instance whose ownership passes to wgpu.
    Some(unsafe { wgpu::Instance::from_hal::<Vulkan>(created) })
}

/// Opens `adapter` with exactly `descriptor`'s features and limits plus DLSS Super Resolution's
/// device extensions, returning whether DLSS can run on the device. `None` means the adapter
/// isn't Vulkan, can't satisfy the descriptor, or failed to open, so the caller requests the
/// device the usual way and gets wgpu's own validation errors.
pub(super) fn request_device(
    adapter: &wgpu::Adapter,
    descriptor: &wgpu::DeviceDescriptor<'_>,
) -> Option<(wgpu::Device, wgpu::Queue, bool)> {
    if !adapter_satisfies(
        descriptor,
        adapter.features(),
        &adapter.limits(),
        descriptor.experimental_features.is_enabled(),
    ) {
        return None;
    }
    // SAFETY: The raw adapter only opens a device that this adapter then adopts.
    let raw = unsafe { adapter.as_hal::<Vulkan>() }?;
    let mut support = FeatureSupport::default();
    let mut registered = Ok(());
    // SAFETY: The adapter supports the requested features and limits, checked above, and the
    // callback only appends device extensions NGX found on this physical device.
    let opened = unsafe {
        raw.open_with_callback(
            descriptor.required_features,
            &descriptor.required_limits,
            &descriptor.memory_hints,
            Some(Box::new(|mut args| {
                registered = dlss_wgpu::register_device_extensions(
                    render::DLSS_PROJECT_ID,
                    &mut args,
                    &raw,
                    &mut support,
                );
            })),
        )
    };
    drop(raw);
    let opened = match opened {
        Ok(opened) => opened,
        Err(error) => {
            log(format_args!(
                "Renderer DLSS Vulkan device failed; requesting the default device: {error}"
            ));
            return None;
        }
    };
    // A device without DLSS's extensions is otherwise the one wgpu would open, so keep it.
    let dlss = match unsupported(registered, &support) {
        Some(reason) => {
            log(format_args!("Renderer DLSS unavailable: {reason}"));
            false
        }
        None => true,
    };
    // SAFETY: `opened` came from this adapter with exactly the descriptor's features and limits.
    match unsafe { adapter.create_device_from_hal(opened, descriptor) } {
        Ok((device, queue)) => Some((device, queue, dlss)),
        Err(error) => {
            log(format_args!(
                "Renderer DLSS device adoption failed; requesting the default device: {error}"
            ));
            None
        }
    }
}

/// Why DLSS Super Resolution can't use the instance or device whose extension registration
/// returned `registered` and `support`, or `None` when its extensions were enabled. Generic
/// because `dlss_wgpu` doesn't export its registration error type.
fn unsupported(
    registered: Result<(), impl std::fmt::Display>,
    support: &FeatureSupport,
) -> Option<String> {
    match registered {
        Err(error) => Some(format!("NGX extension query failed: {error}")),
        Ok(()) if !support.super_resolution_supported => {
            Some("Vulkan lacks the extensions DLSS Super Resolution needs".to_owned())
        }
        Ok(()) => None,
    }
}

/// The checks wgpu's own device request makes before opening, which opening from hal skips:
/// every requested feature and limit is within the adapter's, and experimental features are
/// only requested when enabled.
fn adapter_satisfies(
    descriptor: &wgpu::DeviceDescriptor<'_>,
    features: wgpu::Features,
    limits: &wgpu::Limits,
    experimental_enabled: bool,
) -> bool {
    features.contains(descriptor.required_features)
        && descriptor.required_limits.check_limits(limits)
        && (experimental_enabled
            || !descriptor
                .required_features
                .intersects(wgpu::Features::all_experimental_mask()))
}

#[cfg(test)]
mod tests;
