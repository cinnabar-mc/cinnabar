//! Optional GPU interpolation backend. It never updates game state or presents a surface.
//!
//! Compile with `frame-generation` to use this API. The normal renderer has no caller yet.
//! Prepare during loading, then submit one HUD-less SDR world image per rendered frame.
//! See the adjacent README for input contracts and the remaining presentation work.

mod interpolation;
mod optical_flow;
#[cfg(feature = "frame-generation")]
pub mod runtime;

use glam::Mat4;
use interpolation::{Inputs, Interpolation};
use optical_flow::OpticalFlow;

/// Encoded SDR colour shared by input admission and preallocated real-frame history.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Immutable dimensions for one prepared interpolation context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameGenerationSize {
    /// Native output pixels of the HUD-less colour input and generated image.
    pub display: [u32; 2],
    /// World depth pixels, which may be smaller when Render Resolution is reduced.
    pub render: [u32; 2],
}

impl FrameGenerationSize {
    /// Rejects sizes that the fixed optical-flow pyramid or device cannot represent.
    fn validate(self, limits: &wgpu::Limits) -> Result<(), FrameGenerationError> {
        if self
            .display
            .into_iter()
            .zip(optical_flow::MIN_DISPLAY)
            .any(|(size, minimum)| size < minimum)
        {
            return Err(FrameGenerationError::Invalid(
                "image is too small for the optical-flow pyramid",
            ));
        }
        for size in [self.display, self.render] {
            if size.contains(&0)
                || size.iter().any(|&v| v > limits.max_texture_dimension_2d)
                || u64::from(size[0]) * u64::from(size[1]) > i32::MAX as u64
            {
                return Err(FrameGenerationError::Invalid(
                    "unsupported image dimensions",
                ));
            }
        }
        let flow = self.display.map(|v| v.div_ceil(optical_flow::BLOCK_SIZE));
        for size in [self.render, flow] {
            let field_bytes = u64::from(size[0]) * u64::from(size[1]) * 4;
            if field_bytes > limits.max_storage_buffer_binding_size
                || field_bytes > limits.max_buffer_size
            {
                return Err(FrameGenerationError::Invalid(
                    "motion field exceeds the device's storage buffer limit",
                ));
            }
        }
        Ok(())
    }
}

/// One rendered world image. All values describe the same real frame.
pub struct FrameGenerationInput<'a> {
    /// Full native image, `Rgba8Unorm`, with sRGB-encoded RGB and COPY_SRC | TEXTURE_BINDING.
    /// Screen UI, the first-person hand and sharp world labels must be excluded.
    pub color: &'a wgpu::Texture,
    /// Single-sampled `Depth32Float`, with TEXTURE_BINDING, finite reversed depth.
    pub depth: &'a wgpu::Texture,
    /// Current unjittered clip space to previous clip space, relative to the camera origin.
    pub previous_clip_from_clip: Mat4,
    /// Projection jitter in render pixels, or zero without temporal AA.
    pub jitter: [f32; 2],
    /// Positive near clip distance in blocks.
    pub near: f32,
    /// Far clip distance in blocks, or positive infinity for an infinite reverse-Z projection.
    pub far: f32,
    /// Vertical field of view in radians, between zero and pi.
    pub fov_y: f32,
    /// Discards history after cuts, teleports, dimension changes or other discontinuities.
    pub reset: bool,
}

impl FrameGenerationInput<'_> {
    /// Validates complete textures before creating bindings or modifying GPU history.
    fn validate(&self, size: FrameGenerationSize) -> Result<(), FrameGenerationError> {
        for (texture, expected_size, format, usages) in [
            (
                self.color,
                size.display,
                COLOR_FORMAT,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            ),
            (
                self.depth,
                size.render,
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        ] {
            if texture.width() != expected_size[0]
                || texture.height() != expected_size[1]
                || texture.depth_or_array_layers() != 1
                || texture.dimension() != wgpu::TextureDimension::D2
                || texture.sample_count() != 1
                || texture.mip_level_count() != 1
                || texture.format() != format
                || !texture.usage().contains(usages)
            {
                return Err(FrameGenerationError::Invalid(
                    "input texture does not match the prepared dimensions, format or usages",
                ));
            }
        }
        let determinant = self.previous_clip_from_clip.determinant();
        if !self.previous_clip_from_clip.is_finite()
            || !determinant.is_finite()
            || determinant.abs() <= f32::EPSILON
            || !self.jitter.into_iter().all(f32::is_finite)
            || !self.near.is_finite()
            || self.far.is_nan()
            || self.near <= 0.0
            || self.far <= self.near
            || !self.fov_y.is_finite()
            || self.fov_y <= 0.0
            || self.fov_y >= std::f32::consts::PI
        {
            return Err(FrameGenerationError::Invalid("invalid camera projection"));
        }
        Ok(())
    }
}

/// Preparation or input failure; callers keep presenting ordinary real frames.
#[derive(Debug)]
pub enum FrameGenerationError {
    /// The provided configuration or frame violates the backend's input contract.
    Invalid(&'static str),
    /// GPU allocation or pipeline preparation failed.
    Gpu(String),
}

impl std::fmt::Display for FrameGenerationError {
    /// Describes why this optional backend cannot process the frame.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => f.write_str(reason),
            Self::Gpu(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for FrameGenerationError {}

/// GPU result in sRGB-encoded `Rgba16Float` at native output size, without overlays.
pub struct GeneratedFrame<'a> {
    pub image: &'a wgpu::TextureView,
    /// True when CPU history was reset: do not schedule an extra presentation.
    /// GPU scene-change detection may also replace a candidate with the real image.
    pub history_reset: bool,
}

/// One camera's optical-flow, interpolation and retained real-frame history.
pub struct FrameGenerator {
    size: FrameGenerationSize,
    flow: OpticalFlow,
    interpolation: Interpolation,
    reset: bool,
}

impl FrameGenerator {
    /// Compiles every pipeline and allocates size-dependent storage before first use.
    ///
    /// This operation blocks. Run it during loading or on a worker, never when toggling a
    /// gameplay setting. A changed output or depth size needs a newly prepared context.
    pub fn prepare(
        device: &wgpu::Device,
        size: FrameGenerationSize,
    ) -> Result<Self, FrameGenerationError> {
        size.validate(&device.limits())?;
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let flow = OpticalFlow::new(device, size.display);
        let mut interpolation = Interpolation::new(device, size.display);
        interpolation.prepare_inputs(device, size.render);
        let allocation_error = bevy::tasks::block_on(allocation.pop());
        let validation_error = bevy::tasks::block_on(validation.pop());
        if let Some(error) = allocation_error.or(validation_error) {
            return Err(FrameGenerationError::Gpu(error.to_string()));
        }
        Ok(Self {
            size,
            flow,
            interpolation,
            reset: true,
        })
    }

    /// Drops the temporal relationship without releasing prepared pipelines or storage.
    /// Call whenever a frame is bypassed, even if no image was submitted.
    pub fn invalidate(&mut self) {
        self.reset = true;
    }

    /// Submits GPU work after the real colour and depth producers on this device's queue.
    ///
    /// Own submission keeps each frame's uniform writes ahead of its dispatches. It does
    /// not wait for the GPU, acquire a surface, sample input or advance simulation.
    /// The result is overwritten by the next successful call on this context.
    pub fn generate(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &FrameGenerationInput<'_>,
    ) -> Result<GeneratedFrame<'_>, FrameGenerationError> {
        let was_reset = std::mem::replace(&mut self.reset, true);
        frame.validate(self.size)?;
        let reset = was_reset || frame.reset;
        let color = frame.color.create_view(&Default::default());
        let depth = frame.depth.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame generation"),
        });
        self.flow
            .dispatch(device, queue, &mut encoder, &color, reset);
        self.interpolation.dispatch(
            device,
            queue,
            &mut encoder,
            &Inputs {
                color: &color,
                depth: &depth,
                render_size: self.size.render,
                jitter: frame.jitter,
                reproject: frame.previous_clip_from_clip,
                near: frame.near,
                far: frame.far,
                fov_y: frame.fov_y,
                reset,
                flow_vectors: self.flow.vectors(),
                scene_change: self.flow.scene_change(),
            },
        );
        queue.submit([encoder.finish()]);
        self.reset = false;
        Ok(GeneratedFrame {
            image: self.interpolation.output(),
            history_reset: reset,
        })
    }
}

#[cfg(test)]
#[path = "frame_generation/tests.rs"]
mod tests;
