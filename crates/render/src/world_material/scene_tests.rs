//! A view's scene copy: the opaque colour as drawn, resolved when multisampled, and the depth
//! as linear view distance, reading as far where nothing opaque was drawn.

use super::*;
use crate::scene_sampling::tests::{fixture, texture};
use bevy::render::texture::CachedTexture;

/// Reads the first pixel of a 2×2 texture of `bytes` bytes a texel.
fn first_pixel(
    device: &RenderDevice,
    queue: &wgpu::Queue,
    encoder: wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    bytes: u32,
) -> Vec<u8> {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene copy readback"),
        size: 512,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = encoder;
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(2),
            },
        },
        wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    slice.get_mapped_range().unwrap()[..bytes as usize].to_vec()
}

#[test]
fn the_copy_holds_the_opaque_colour_and_linear_depth() {
    let Some((device, queue, _)) = fixture() else {
        return;
    };
    for samples in [1, 4] {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | if samples == 1 {
                wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::empty()
            };
        let colour = texture(&device, format, samples, usage);
        let colour_view = colour.create_view(&Default::default());
        let depth_texture = texture(
            &device,
            wgpu::TextureFormat::Depth32Float,
            samples,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = ViewDepthStencilTexture::new(
            CachedTexture {
                default_view: depth_texture.create_view(&Default::default()),
                texture: depth_texture.clone(),
            },
            Some(0.0),
            None,
        );
        // The opaque pass: one colour, and depth one quarter of the way in reverse Z.
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("opaque pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.0,
                        g: 0.5,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_texture.create_view(&Default::default()),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.25),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let source = Source {
            color: colour_view.id(),
            depth: crate::scene_sampling::view_depth(&depth).id(),
            size: colour.size(),
            format,
            read_format: format,
            samples,
        };
        let layout = device.create_bind_group_layout(
            Some("scene layout"),
            &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        );
        let copy = SceneCopy::new(&device, &layout, source, &depth).unwrap();
        queue.write_buffer(
            &copy.projection,
            0,
            bytemuck::cast_slice(&[0.05_f32, 0.0, 0.0, 0.0]),
        );
        copy.encode(&mut encoder, &colour, &colour_view);
        let pixel = first_pixel(&device, &queue, encoder, &copy.color, 4);
        assert_eq!(pixel, [255, 128, 0, 255], "{samples} samples");
        let encoder = device.create_command_encoder(&Default::default());
        let depth = first_pixel(&device, &queue, encoder, &copy._depth, 4);
        let distance = f32::from_le_bytes(depth.try_into().unwrap());
        assert!(
            (distance - 0.2).abs() < 1.0e-4,
            "{samples} samples: {distance}"
        );
    }
}
