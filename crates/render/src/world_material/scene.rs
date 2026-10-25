//! The scene a world material reads (`render_world_material_scene`): each view's opaque
//! colour and linear depth, copied once the opaque pass has drawn them and before the
//! translucent pass that blended world materials draw in, in a bind group of its own.
//! Nothing here is ever read back to a guest.

use bevy::prelude::{Entity, Query, Res, ResMut, Resource, World};
use bevy::render::{
    render_resource::{
        BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
        BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferDescriptor, BufferUsages,
        Extent3d, PipelineCache, ShaderStages, Texture, TextureDescriptor, TextureDimension,
        TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
        TextureViewDimension, TextureViewId,
    },
    renderer::{RenderContext, RenderDevice, RenderQueue},
    view::{ExtractedView, ViewDepthStencilTexture, ViewTarget},
};
use std::collections::HashMap;

use crate::scene_target::SceneTarget;

/// The format of the linear depth a material reads.
const DEPTH_FORMAT: TextureFormat = TextureFormat::R32Float;

/// The layout of the scene group: the opaque colour at binding 0 and its linear depth at
/// binding 1, both read with `textureLoad` by fragment stages.
pub(crate) fn scene_layout() -> BindGroupLayoutDescriptor {
    let texture = |binding| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    BindGroupLayoutDescriptor::new("world material scene layout", &[texture(0), texture(1)])
}

/// Where a view's opaque colour comes from: the shared scene colour or the view's main
/// texture, with its sample count and the format a material reads it in.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Source {
    color: TextureViewId,
    depth: TextureViewId,
    size: Extent3d,
    format: TextureFormat,
    read_format: TextureFormat,
    samples: u32,
}

/// One view's copies and the bind group materials read them through.
pub(crate) struct SceneCopy {
    source: Source,
    color: Texture,
    /// The copy's view in its own format, a resolve's target.
    color_target: TextureView,
    depth_target: TextureView,
    projection: Buffer,
    depth_pipeline: wgpu::RenderPipeline,
    depth_binding: wgpu::BindGroup,
    group: BindGroup,
    _depth: Texture,
}

/// Every view's scene copy, made only while a published material reads the scene.
#[derive(Resource, Default)]
pub(crate) struct WorldSceneCopies(HashMap<Entity, SceneCopy>);

impl WorldSceneCopies {
    /// The scene group of `view`, once its copies exist.
    pub(crate) fn group(&self, view: Entity) -> Option<&BindGroup> {
        self.0.get(&view).map(|copy| &copy.group)
    }
}

/// The texture a view's opaque pass draws into, its view identity and the format materials
/// read it in.
fn source_color<'a>(
    target: &'a ViewTarget,
    scene: Option<&'a SceneTarget>,
) -> (&'a Texture, TextureViewId, TextureFormat) {
    match scene {
        Some(scene) => (&scene.texture, scene.texture_view_id(), scene.read_format()),
        None => (
            target.main_texture(),
            target.main_texture_view().id(),
            target.main_texture_format(),
        ),
    }
}

/// Creates, resizes or drops each view's scene copy: views get one only while a published
/// material reads the scene and their depth can be sampled. Writes the near plane its depth
/// pass divides by.
pub(crate) fn prepare(
    gpu: Option<Res<super::WorldMaterialGpu>>,
    views: Query<(
        Entity,
        &ViewTarget,
        Option<&SceneTarget>,
        &ViewDepthStencilTexture,
        &ExtractedView,
    )>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut copies: ResMut<WorldSceneCopies>,
) {
    if !gpu.is_some_and(|gpu| gpu.reads_scene()) {
        copies.0.clear();
        return;
    }
    let mut live = Vec::new();
    for (entity, target, scene, depth, view) in &views {
        if !depth
            .texture()
            .usage()
            .contains(TextureUsages::TEXTURE_BINDING)
        {
            continue;
        }
        let (texture, color, read_format) = source_color(target, scene);
        let source = Source {
            color,
            depth: crate::scene_sampling::view_depth(depth).id(),
            size: texture.size(),
            format: texture.format(),
            read_format,
            samples: texture.sample_count(),
        };
        live.push(entity);
        if copies
            .0
            .get(&entity)
            .is_none_or(|copy| copy.source != source)
            && let Some(copy) = SceneCopy::new(
                &device,
                &cache.get_bind_group_layout(&scene_layout()),
                source,
                depth,
            )
        {
            copies.0.insert(entity, copy);
        }
        if let Some(copy) = copies.0.get(&entity) {
            let near = view.clip_from_view.w_axis.z;
            queue.write_buffer(
                &copy.projection,
                0,
                bytemuck::cast_slice(&[near, 0.0, 0.0, 0.0]),
            );
        }
    }
    copies.0.retain(|entity, _| live.contains(entity));
}

impl SceneCopy {
    /// The copies of a view whose opaque colour is `source` and depth `depth`, read through a
    /// group of `layout` ([`scene_layout`]); `None` when the device is out of memory.
    fn new(
        device: &RenderDevice,
        layout: &BindGroupLayout,
        source: Source,
        depth: &ViewDepthStencilTexture,
    ) -> Option<Self> {
        let size = Extent3d {
            depth_or_array_layers: 1,
            ..source.size
        };
        let pixels = u64::from(size.width) * u64::from(size.height);
        let colour_bytes = pixels * u64::from(source.format.block_copy_size(None).unwrap_or(8));
        let read_formats = [source.read_format];
        let color = crate::gpu_allocation::checked(
            device,
            "world material scene colour",
            colour_bytes,
            || {
                device.create_texture(&TextureDescriptor {
                    label: Some("world material scene colour"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: source.format,
                    // Copy source too, so captures and tests can read what materials read.
                    usage: TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_DST
                        | TextureUsages::COPY_SRC
                        | TextureUsages::RENDER_ATTACHMENT,
                    view_formats: if source.read_format == source.format {
                        &[]
                    } else {
                        &read_formats
                    },
                })
            },
        )?;
        let depth_texture = crate::gpu_allocation::checked(
            device,
            "world material scene depth",
            pixels * 4,
            || {
                device.create_texture(&TextureDescriptor {
                    label: Some("world material scene depth"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: DEPTH_FORMAT,
                    usage: TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_SRC
                        | TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
            },
        )?;
        let color_target = color.create_view(&TextureViewDescriptor::default());
        let color_read = color.create_view(&TextureViewDescriptor {
            format: Some(source.read_format),
            ..Default::default()
        });
        let depth_target = depth_texture.create_view(&TextureViewDescriptor::default());
        let projection = device.create_buffer(&BufferDescriptor {
            label: Some("world material scene projection"),
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gpu = device.wgpu_device();
        let shader = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world material scene depth"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene_depth.wgsl").into()),
        });
        let multi = depth.texture().sample_count() > 1;
        let depth_pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world material scene depth"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if multi { "multisampled" } else { "single" }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DEPTH_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let depth_binding = gpu.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world material scene depth input"),
            layout: &depth_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: u32::from(multi),
                    resource: wgpu::BindingResource::TextureView(
                        crate::scene_sampling::view_depth(depth),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: projection.as_entire_binding(),
                },
            ],
        });
        let group = device.create_bind_group(
            Some("world material scene"),
            layout,
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&color_read),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&depth_target),
                },
            ],
        );
        Some(Self {
            source,
            color,
            color_target,
            depth_target,
            projection,
            depth_pipeline,
            depth_binding,
            group,
            _depth: depth_texture,
        })
    }

    /// Copies the view's opaque colour (resolving it when multisampled) and writes its linear
    /// depth.
    fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &Texture,
        color_view: &TextureView,
    ) {
        if self.source.samples > 1 {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world material scene colour resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: Some(&self.color_target),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        } else {
            encoder.copy_texture_to_texture(
                color.as_image_copy(),
                self.color.as_image_copy(),
                Extent3d {
                    depth_or_array_layers: 1,
                    ..self.source.size
                },
            );
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world material scene depth"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.depth_target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.depth_pipeline);
        pass.set_bind_group(0, &self.depth_binding, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Copies the view's opaque scene for the world materials that read it, after the opaque pass
/// and before the translucent one.
pub(crate) fn copy_scene(
    world: &World,
    query: bevy::render::renderer::ViewQuery<(&'static ViewTarget, Option<&'static SceneTarget>)>,
    mut context: RenderContext,
) {
    let entity = query.entity();
    let Some(copy) = world
        .get_resource::<WorldSceneCopies>()
        .and_then(|copies| copies.0.get(&entity))
    else {
        return;
    };
    let (target, scene) = query.into_inner();
    let (texture, id, _) = source_color(target, scene);
    if id != copy.source.color {
        return;
    }
    let view = match scene {
        Some(scene) => scene.encoded_view(),
        None => target.main_texture_view(),
    };
    copy.encode(context.command_encoder(), texture, view);
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
