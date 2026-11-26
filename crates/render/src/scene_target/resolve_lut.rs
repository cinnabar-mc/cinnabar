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

/// Builds the table on devices that support two-sample resolves.
pub(super) fn build(
    mut commands: Commands,
    adapter: Res<RenderAdapter>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if supported(&adapter, &device) {
        commands.insert_resource(ResolveLut {
            view: table(device.wgpu_device(), &queue).into(),
        });
    }
}

/// Encodes and submits the table's construction, returning its sRGB view.
pub(crate) fn table(gpu: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
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
        label: Some("two-sample resolve table"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let module = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("two-sample resolve table"),
        source: wgpu::ShaderSource::Wgsl(SOURCE.into()),
    });
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
    let encoded = samples.create_view(&Default::default());
    let linear = samples.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ..Default::default()
    });
    let view = table.create_view(&Default::default());
    let mut encoder = gpu.create_command_encoder(&Default::default());
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
        label: Some("two-sample resolve table"),
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
    queue.submit([encoder.finish()]);
    view
}

#[cfg(test)]
#[path = "resolve_lut_tests.rs"]
mod tests;
