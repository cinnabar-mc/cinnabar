//! Turns a graphics driver's failure to build one shader module or pipeline into a value its
//! owner handles, so the failure never reaches the device's uncaptured-error handler.

use std::fmt;

/// wgpu starts every error description with this, whatever the error's kind.
const WGPU_PREAMBLE: &str = "Validation Error\n\nCaused by:\n";

/// The driver's report for a shader module or pipeline it could not build from valid input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverFailure(String);

impl DriverFailure {
    /// The failing call, the object's label and the driver's result; see [`report`].
    pub fn report(&self) -> &str {
        report(&self.0)
    }
}

/// A wgpu error description without the preamble that calls every error a validation error,
/// leaving the failing call, the object's label and the driver's result.
pub fn report(description: &str) -> &str {
    description
        .strip_prefix(WGPU_PREAMBLE)
        .unwrap_or(description)
        .trim()
}

impl fmt::Display for DriverFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.report())
    }
}

impl std::error::Error for DriverFailure {}

/// Runs `create` and returns the internal error `device` raised while it ran, if any.
///
/// * Only [`wgpu::Error::Internal`] is captured: a driver or backend compiler rejecting a
///   valid shader module or pipeline. Validation and out-of-memory errors still reach the
///   caller's own scopes or the device's uncaptured-error handler.
/// * Error scopes belong to a thread, so `create` must make its device calls on the calling
///   thread. It may push and pop scopes of its own.
/// * On `Err`, everything `create` built is dropped, because at least one of those objects is
///   invalid. The first failure is the one reported.
/// * `create` should end with the pipelines it builds. Using a failed object in a later device
///   call, such as asking a pipeline for its bind group layout, raises a validation error that
///   is not captured here; build dependents after checking the result.
/// * On wasm32 nothing is captured and the result is always `Ok`: a browser's scope cannot
///   settle before this call returns, so the error reaches the uncaptured-error handler.
pub fn catch<T>(device: &wgpu::Device, create: impl FnOnce() -> T) -> Result<T, DriverFailure> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = device;
        Ok(create())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::future::Future as _;
        use std::task::{Context, Poll, Waker};

        let scope = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let created = create();
        // Native wgpu resolves a popped scope immediately, so one poll reads it.
        let popped = std::pin::pin!(scope.pop());
        match popped.poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(Some(wgpu::Error::Internal { description, .. })) => {
                Err(DriverFailure(description))
            }
            _ => Ok(created),
        }
    }
}

/// Makes the noop backend fail pipelines the way a driver does.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use wgpu::hal::noop::PipelineFailure;

    /// The driver's message in the report of a failure [`fail_next_pipeline`] injected.
    pub const DRIVER_MESSAGE: &str = PipelineFailure::DRIVER_MESSAGE;

    /// Makes the next render or compute pipeline `device` creates fail with a driver error;
    /// later ones succeed again. `device` must come from `wgpu::Device::noop`.
    pub fn fail_next_pipeline(device: &wgpu::Device) {
        // SAFETY: The backend device only records the request; no wgpu-owned state is touched.
        let backend = unsafe { device.as_hal::<wgpu::hal::api::Noop>() }
            .expect("driver failures can only be injected into a noop device");
        backend.fail_next_pipeline_creation(PipelineFailure::Driver);
    }

    /// A one-triangle shader with `vertex`, `fragment` and `compute` entry points.
    pub const SHADER: &str = "
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    return vec4f(f32(index), 0., 0., 1.);
}
@fragment fn fragment() -> @location(0) vec4f { return vec4f(1.); }
@compute @workgroup_size(1) fn compute() {}
";

    /// Creates a labelled render pipeline from [`SHADER`], as an owner's direct call would.
    pub fn render_pipeline(device: &wgpu::Device, label: &str) -> wgpu::RenderPipeline {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{DRIVER_MESSAGE, fail_next_pipeline, render_pipeline};
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A noop device that records every error no scope captured.
    fn device() -> (wgpu::Device, Arc<Mutex<Vec<String>>>) {
        let (device, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let uncaptured = Arc::new(Mutex::new(Vec::new()));
        let recorded = uncaptured.clone();
        device.on_uncaptured_error(Arc::new(move |error| {
            recorded.lock().unwrap().push(error.to_string());
        }));
        (device, uncaptured)
    }

    #[test]
    fn a_driver_failure_is_returned_instead_of_escaping() {
        let (device, uncaptured) = device();
        fail_next_pipeline(&device);
        let failure = catch(&device, || render_pipeline(&device, "terrain"))
            .expect_err("the injected failure must be returned");
        let report = failure.report();
        assert!(report.contains("terrain"), "{report}");
        assert!(report.contains(DRIVER_MESSAGE), "{report}");
        assert!(!report.contains("Validation Error"), "{report}");
        assert_eq!(failure.to_string(), report);
        assert!(uncaptured.lock().unwrap().is_empty());

        // The device survives: the same pipeline builds on the next attempt.
        assert!(catch(&device, || render_pipeline(&device, "terrain")).is_ok());
        assert!(uncaptured.lock().unwrap().is_empty());
    }

    #[test]
    fn a_worker_thread_captures_its_own_failure() {
        let (device, uncaptured) = device();
        fail_next_pipeline(&device);
        let worker = device.clone();
        let result = std::thread::spawn(move || {
            catch(&worker, || render_pipeline(&worker, "worker")).map(drop)
        })
        .join()
        .unwrap();
        assert!(result.is_err());
        assert!(uncaptured.lock().unwrap().is_empty());
    }

    #[test]
    fn other_errors_still_reach_their_own_handler() {
        let (device, uncaptured) = device();
        let texture = |size| wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        };
        assert!(catch(&device, || device.create_texture(&texture(0))).is_ok());
        assert_eq!(uncaptured.lock().unwrap().len(), 1);

        // A scope the caller pushed inside `create` keeps working.
        let inner = catch(&device, || {
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let _texture = device.create_texture(&texture(0));
            scope.pop()
        })
        .unwrap();
        let mut inner = std::pin::pin!(inner);
        let poll = std::future::Future::poll(
            inner.as_mut(),
            &mut std::task::Context::from_waker(std::task::Waker::noop()),
        );
        assert!(matches!(
            poll,
            std::task::Poll::Ready(Some(wgpu::Error::Validation { .. }))
        ));
        assert_eq!(uncaptured.lock().unwrap().len(), 1);
    }
}
