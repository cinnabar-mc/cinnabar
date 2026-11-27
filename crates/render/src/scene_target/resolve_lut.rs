//! The device's own two-sample resolve, tabulated so a shader can resolve scene samples with the
//! exact bytes a render-pass resolve would write.
//!
//! Texel `(x, y)` holds the resolve of a pixel whose first sample is the code `x` and whose
//! second is `y` in every channel, produced by the same kind of resolve as the scene's: a
//! render pass resolving a two-sample `Rgba8Unorm` image through its sRGB view into an sRGB
//! target. Resolves treat channels independently, so a shader that resolves channel `c` from
//! the texel at `(sample0.c, sample1.c)` reproduces the device's result for any pixel.

use bevy::{
    prelude::*,
    render::renderer::{RenderAdapter, RenderDevice, RenderQueue},
};

/// Codes per channel of an eight-bit target.
const CODES: u32 = 256;

const SOURCE: &str = "
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return vec4(vec2(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0, 0.0, 1.0);
}
// Sample 0 takes the column's code and sample 1 the row's, written exactly into UNORM bytes.
@fragment fn samples(@builtin(position) position: vec4<f32>, @builtin(sample_index) sample: u32)
    -> @location(0) vec4<f32> {
    return vec4(select(floor(position.x), floor(position.y), sample == 1u) / 255.0);
}";

/// The tabulated two-sample resolve, once built.
#[derive(Resource)]
pub(crate) struct ResolveLut {
    /// `256 x 256` `Rgba8UnormSrgb`: sRGB-resolved colour in every colour channel and the
    /// linearly resolved alpha in alpha.
    pub(crate) view: bevy::render::render_resource::TextureView,
}

/// Whether the device can resolve two-sample scene colour, which the table needs.
fn supported(adapter: &RenderAdapter, device: &RenderDevice) -> bool {
    device
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        && [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ]
        .iter()
        .all(|format| {
            let flags = adapter.get_texture_format_features(*format).flags;
            flags.sample_count_supported(2)
                && flags.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE)
        })
}

/// Whether the table can be built on this device, and its failed builds still backing off.
#[derive(Resource, Default)]
pub(super) struct ResolveLutBuilder {
    supported: bool,
    failed: crate::gpu_allocation::FailedRequests<()>,
}

/// Records whether the device supports the table; nothing is allocated until a view needs it.
pub(super) fn init(mut commands: Commands, adapter: Res<RenderAdapter>, device: Res<RenderDevice>) {
    commands.insert_resource(ResolveLutBuilder {
        supported: supported(&adapter, &device),
        failed: Default::default(),
    });
}

/// Builds the table once a two-sample view needs it. A failed allocation leaves the table
/// absent, which keeps the plain resolve, and retries with backoff while a view still needs it.
pub(super) fn build(
    mut commands: Commands,
    mut builder: ResMut<ResolveLutBuilder>,
    views: Query<&Msaa>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if !builder.supported || !views.iter().any(|msaa| *msaa == Msaa::Sample2) {
        return;
    }
    if let Some(view) = builder
        .failed
        .attempt(Entity::PLACEHOLDER, (), || table(&device, &queue))
    {
        commands.insert_resource(ResolveLut { view: view.into() });
    }
}

/// Label of the table's build, for its error scopes and allocation warnings.
const BUILD_LABEL: &str = "two-sample resolve table";

/// Steps of the table's build, in order; tests raise a real GPU error after any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Textures,
    Views,
    Module,
    Pipeline,
    Encoder,
    Passes,
    Finish,
}

#[cfg(test)]
thread_local! {
    /// The step after which this thread's next build raises a validation error.
    pub(super) static FAULT_AFTER: std::cell::Cell<Option<Step>> = const { std::cell::Cell::new(None) };
    /// Builds this thread submitted.
    pub(super) static SUBMITTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Does nothing outside tests, which may raise a GPU error after any step.
#[cfg(not(test))]
fn fault_after(_: &wgpu::Device, _: Step) {}

/// Raises a real validation error after `step` when a test asked for one.
#[cfg(test)]
fn fault_after(gpu: &wgpu::Device, step: Step) {
    if FAULT_AFTER.get() == Some(step) {
        // A mapped buffer's size must be a multiple of four.
        let _ = gpu.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resolve table test fault"),
            size: 3,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
    }
}

/// Builds the table and submits its construction, returning its sRGB view; `None`, with
/// nothing submitted, when any step of the build failed.
///
/// Every step, from the first texture through `finish`, runs inside one pair of
/// out-of-memory and validation error scopes ([`crate::gpu_allocation::fallible`]): a step
/// that runs out of memory returns an invalid handle, and a later step given that handle
/// fails validation, so both kinds are captured and none reaches the uncaptured error handler.
/// The finished commands are submitted only once both scopes come back clean.
pub(crate) fn table(device: &RenderDevice, queue: &wgpu::Queue) -> Option<wgpu::TextureView> {
    let bytes = u64::from(CODES) * u64::from(CODES) * 4 * 3;
    let (view, commands) = crate::gpu_allocation::fallible(device, BUILD_LABEL, bytes, || {
        encode(device.wgpu_device())
    })?;
    #[cfg(test)]
    SUBMITTED.set(SUBMITTED.get() + 1);
    queue.submit([commands]);
    Some(view)
}

/// Creates the table's resources and encodes its construction, without submitting it.
fn encode(gpu: &wgpu::Device) -> (wgpu::TextureView, wgpu::CommandBuffer) {
    let size = wgpu::Extent3d {
        width: CODES,
        height: CODES,
        depth_or_array_layers: 1,
    };
    let samples = gpu.create_texture(&wgpu::TextureDescriptor {
        label: Some("two-sample resolve table samples"),
        size,
        mip_level_count: 1,
        sample_count: 2,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
    });
    let table = gpu.create_texture(&wgpu::TextureDescriptor {
        label: Some(BUILD_LABEL),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    fault_after(gpu, Step::Textures);
    let encoded = samples.create_view(&Default::default());
    let linear = samples.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ..Default::default()
    });
    let view = table.create_view(&Default::default());
    fault_after(gpu, Step::Views);
    let module = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(BUILD_LABEL),
        source: wgpu::ShaderSource::Wgsl(SOURCE.into()),
    });
    fault_after(gpu, Step::Module);
    let pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("two-sample resolve table samples"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 2,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("samples"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    fault_after(gpu, Step::Pipeline);
    let mut encoder = gpu.create_command_encoder(&Default::default());
    fault_after(gpu, Step::Encoder);
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("two-sample resolve table samples"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &encoded,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    // The scene's own resolve: a load of the samples through their sRGB view.
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(BUILD_LABEL),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &linear,
            depth_slice: None,
            resolve_target: Some(&view),
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Discard,
            },
        })],
        ..Default::default()
    });
    fault_after(gpu, Step::Passes);
    let commands = encoder.finish();
    fault_after(gpu, Step::Finish);
    (view, commands)
}

#[cfg(test)]
#[path = "resolve_lut_tests.rs"]
mod tests;
