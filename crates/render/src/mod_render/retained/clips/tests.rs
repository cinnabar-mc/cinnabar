use super::*;
use crate::gpu_snapshot::Gpu as Fixture;
use wgpu::util::DeviceExt;

#[test]
fn portal_resources_retry_a_resize_after_old_attachment_storage_retires() {
    let bytes = 2560 * 1440 * 4 * ATTACHMENT_SAMPLE_BYTES;
    let ledger = mod_render::GpuLedger::new(mod_api::drawing::MAX_CLIP_ATTACHMENT_BYTES);
    let old = ledger.allocate(bytes, || Some(())).unwrap();
    assert!(reserve_attachment(&ledger, bytes, bytes).unwrap().is_none());
    drop(old);
    let replacement = reserve_attachment(&ledger, bytes, 0).unwrap().unwrap();
    assert_eq!(ledger.used(), bytes);
    assert!(reserve_attachment(&ledger, ledger.limit() + 1, 0).is_err());
    drop(replacement);
    assert_eq!(ledger.used(), 0);
}

/// Uses the production clip shader and stencil state against deterministic tiny attachments.
#[test]
fn masks_hide_occluded_pixels_intersect_parents_and_isolate_content_depth() {
    let Some(gpu) = Fixture::for_fixture("mesh clipping") else {
        return;
    };
    let device = &gpu.device;
    let size = wgpu::Extent3d {
        width: 8,
        height: 4,
        depth_or_array_layers: 1,
    };
    let texture = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let color = texture(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let world = texture(
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let private = texture(FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let (color_view, world_view, private_view) = (
        color.create_view(&default()),
        world.create_view(&default()),
        private.create_view(&default()),
    );
    let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(
            r#"
@group(0) @binding(0) var<uniform> parameters: array<vec4<f32>,2>;
@vertex fn vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=array<vec2<f32>,3>(vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    return vec4(p[index],parameters[0].x,1.0);
}
@fragment fn fragment()->@location(0) vec4<f32> {return parameters[1];}
"#
            .into(),
        ),
    });
    let mask = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source(false, include_str!("../clip_mask.wgsl")).into()),
    });
    let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let world_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &depth_layout(false).entries,
    });
    let content_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&uniform_layout)],
        immediate_size: 0,
    });
    let mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&uniform_layout), Some(&world_layout)],
        immediate_size: 0,
    });
    let key = pipeline::Key {
        msaa: Msaa::Off,
        hdr: false,
        always: false,
        additive: false,
        clipped: true,
    };
    let mut mask_descriptor = RenderPipelineDescriptor {
        fragment: Some(FragmentState {
            targets: vec![Some(pipeline::target(key))],
            ..default()
        }),
        ..default()
    };
    MaskSpecialization
        .specialize(key, &mut mask_descriptor)
        .unwrap();
    let make_pipeline = |layout, fragment: &wgpu::ShaderModule, entry, depth, write| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("vertex"),
                compilation_options: default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: fragment,
                entry_point: Some(entry),
                compilation_options: default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: write,
                })],
            }),
            primitive: default(),
            depth_stencil: Some(depth),
            multisample: default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let mask_pipeline = make_pipeline(
        &mask_layout,
        &mask,
        "clip_mask_fragment",
        mask_descriptor.depth_stencil.unwrap(),
        wgpu::ColorWrites::empty(),
    );
    let content_pipeline = make_pipeline(
        &content_layout,
        &vertex,
        "fragment",
        pipeline::depth(key),
        wgpu::ColorWrites::ALL,
    );
    let world_depth = wgpu::DepthStencilState {
        format: wgpu::TextureFormat::Depth32Float,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Always),
        stencil: default(),
        bias: default(),
    };
    let world_pipeline = make_pipeline(
        &content_layout,
        &vertex,
        "fragment",
        world_depth,
        wgpu::ColorWrites::empty(),
    );
    let binding = |depth: f32, color: [f32; 4]| {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[[depth, 0., 0., 0.], color]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        })
    };
    let (foreground, plane, near, far) = (
        binding(0.8, [0.; 4]),
        binding(0.5, [0.; 4]),
        binding(0.3, [1., 0., 0., 1.]),
        binding(0.2, [0., 0., 1., 1.]),
    );
    let world_binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &world_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&world_view),
        }],
    });
    let mut encoder = device.create_command_encoder(&default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &world_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.4),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&world_pipeline);
        pass.set_bind_group(0, &foreground, &[]);
        pass.set_scissor_rect(0, 0, 2, 4);
        pass.draw(0..3, 0..1);
    }
    for nested in [true, false] {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &private_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Store,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&mask_pipeline);
        pass.set_bind_group(0, &plane, &[]);
        pass.set_bind_group(1, &world_binding, &[]);
        pass.set_stencil_reference(0);
        if !nested {
            pass.set_scissor_rect(6, 0, 2, 4);
        }
        pass.draw(0..3, 0..1);
        if nested {
            pass.set_stencil_reference(1);
            pass.set_scissor_rect(0, 0, 6, 4);
            pass.draw(0..3, 0..1);
        }
        pass.set_scissor_rect(0, 0, 8, 4);
        pass.set_pipeline(&content_pipeline);
        pass.set_stencil_reference(if nested { 2 } else { 1 });
        if nested {
            pass.set_bind_group(0, &near, &[]);
            pass.draw(0..3, 0..1);
        }
        pass.set_bind_group(0, &far, &[]);
        pass.draw(0..3, 0..1);
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4 * 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &color,
            mip_level: 0,
            origin: default(),
            aspect: default(),
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(4),
            },
        },
        size,
    );
    encoder.map_buffer_on_submit(&readback, wgpu::MapMode::Read, .., |result| result.unwrap());
    gpu.queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let pixels = readback.slice(..).get_mapped_range().unwrap();
    for y in 0..4 {
        for x in 0..8 {
            assert_eq!(
                &pixels[y * 256 + x * 4..y * 256 + x * 4 + 4],
                if x < 2 {
                    &[0, 0, 0, 0]
                } else if x < 6 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                }
            );
        }
    }
}
