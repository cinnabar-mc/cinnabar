//! World material bloom and outlines (`render_world_material_passes`). After the translucent
//! pass, each view draws the targets of materials with a bloom or an outline once more into
//! two effects targets, depth-tested against what is already drawn: their emission times the
//! bloom strength, and their outline colour and width. Two fullscreen passes then blur the
//! emission and grow the outlines around what can be seen of each target, and blend both over
//! the scene. The work per view is bounded by the redrawn targets and by fixed tap counts.

use bevy::prelude::{Entity, Query, Res, ResMut, Resource, World};
use bevy::render::{
    render_resource::{
        BlendState, ColorTargetState, ColorWrites, CompareFunction, Extent3d,
        RenderPipelineDescriptor, StoreOp, Texture, TextureDescriptor, TextureDimension,
        TextureFormat, TextureUsages, TextureView,
    },
    renderer::{RenderContext, RenderDevice, RenderQueue},
    view::{ViewDepthStencilTexture, ViewTarget},
};
use std::collections::HashMap;

use crate::scene_target::SceneTarget;

/// The bloom source's format: emission times strength, past one.
pub(crate) const EMISSION_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// The outline source's format: colour over sixteen, width over eight pixels.
pub(crate) const OUTLINE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
/// The format of the horizontal pass's results.
const ROW_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Turns a world material pipeline into its effects-pass form: the effects fragment entry
/// writing both effects targets, depth-tested against the scene without writing it. The
/// vertex stage takes the effects definition too when it is the material's module.
pub(crate) fn specialize(descriptor: &mut RenderPipelineDescriptor, vertex_is_material: bool) {
    let def = bevy::shader::ShaderDefVal::Bool("WORLD_MATERIAL_EFFECTS".into(), true);
    descriptor.label = Some("world material effects pipeline".into());
    if vertex_is_material {
        descriptor.vertex.shader_defs.push(def.clone());
    }
    let fragment = descriptor
        .fragment
        .as_mut()
        .expect("world material pipelines draw colour");
    fragment.shader_defs.push(def);
    fragment.entry_point = Some("world_effects_fragment".into());
    let target = |format| {
        Some(ColorTargetState {
            format,
            blend: None,
            write_mask: ColorWrites::ALL,
        })
    };
    fragment.targets = vec![target(EMISSION_FORMAT), target(OUTLINE_FORMAT)];
    descriptor.multisample.alpha_to_coverage_enabled = false;
    if let Some(depth) = descriptor.depth_stencil.as_mut() {
        depth.depth_write_enabled = Some(false);
        // Reverse Z: a target drawn where it already drew passes, an occluded one fails.
        depth.depth_compare = Some(CompareFunction::GreaterEqual);
    }
}

/// Two textures of a view's size: one with the depth's samples to draw into, and its
/// single-sample resolve (the same texture when unsampled).
struct Target {
    drawn: TextureView,
    resolved: Option<TextureView>,
    _textures: Vec<Texture>,
}

impl Target {
    fn new(device: &RenderDevice, size: Extent3d, samples: u32, format: TextureFormat) -> Self {
        let texture = |samples| {
            device.create_texture(&TextureDescriptor {
                label: Some("world material effects target"),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_SRC
                    | TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let drawn = texture(samples);
        if samples == 1 {
            return Self {
                drawn: drawn.create_view(&Default::default()),
                resolved: None,
                _textures: vec![drawn],
            };
        }
        let resolved = texture(1);
        Self {
            drawn: drawn.create_view(&Default::default()),
            resolved: Some(resolved.create_view(&Default::default())),
            _textures: vec![drawn, resolved],
        }
    }

    /// The single-sample view the fullscreen passes read.
    fn read(&self) -> &TextureView {
        self.resolved.as_ref().unwrap_or(&self.drawn)
    }

    fn attachment(&self) -> wgpu::RenderPassColorAttachment<'_> {
        wgpu::RenderPassColorAttachment {
            view: &self.drawn,
            depth_slice: None,
            resolve_target: self.resolved.as_deref(),
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        }
    }
}

/// One view's effects targets, the horizontal pass's results and the fullscreen pipelines.
pub(crate) struct EffectsView {
    size: Extent3d,
    samples: u32,
    emission: Target,
    outline: Target,
    bloom_row: Target,
    outline_row: Target,
    reach: wgpu::Buffer,
    row_pipeline: wgpu::RenderPipeline,
    row_group: wgpu::BindGroup,
    /// The final pass for the view's scene colour format and samples.
    composite: Option<((TextureFormat, u32), wgpu::RenderPipeline, wgpu::BindGroup)>,
    shader: wgpu::ShaderModule,
}

impl EffectsView {
    fn new(device: &RenderDevice, size: Extent3d, samples: u32) -> Self {
        let gpu = device.wgpu_device();
        let shader = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world material effects"),
            source: wgpu::ShaderSource::Wgsl(include_str!("effects.wgsl").into()),
        });
        let emission = Target::new(device, size, samples, EMISSION_FORMAT);
        let outline = Target::new(device, size, samples, OUTLINE_FORMAT);
        let bloom_row = Target::new(device, size, 1, ROW_FORMAT);
        let outline_row = Target::new(device, size, 1, ROW_FORMAT);
        let reach = gpu.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world material effects reach"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let target = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let row_pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world material effects rows"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("row"),
                compilation_options: Default::default(),
                targets: &[target(ROW_FORMAT), target(ROW_FORMAT)],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let row_group = gpu.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world material effects rows"),
            layout: &row_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(emission.read()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(outline.read()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: reach.as_entire_binding(),
                },
            ],
        });
        Self {
            size,
            samples,
            emission,
            outline,
            bloom_row,
            outline_row,
            reach,
            row_pipeline,
            row_group,
            composite: None,
            shader,
        }
    }

    /// Encodes the horizontal pass and the final pass, which blends the bloom and outlines
    /// into `scene`, a colour attachment of `format` and samples; skipped when the final pass
    /// was made for another.
    fn encode_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: wgpu::RenderPassColorAttachment<'_>,
        format: (TextureFormat, u32),
    ) {
        let Some((key, pipeline, group)) = &self.composite else {
            return;
        };
        if *key != format {
            return;
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world material effects rows"),
                color_attachments: &[
                    Some(self.bloom_row.attachment()),
                    Some(self.outline_row.attachment()),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.row_pipeline);
            pass.set_bind_group(0, &self.row_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world material effects composite"),
            color_attachments: &[Some(scene)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Makes the final pass for a scene colour of `format` with `samples`, unless it exists.
    fn ensure_composite(&mut self, device: &RenderDevice, format: TextureFormat, samples: u32) {
        if self
            .composite
            .as_ref()
            .is_some_and(|(key, _, _)| *key == (format, samples))
        {
            return;
        }
        let gpu = device.wgpu_device();
        let pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world material effects composite"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("composite"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let group = gpu.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world material effects composite"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.reach.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(self.bloom_row.read()),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(self.outline_row.read()),
                },
            ],
        });
        self.composite = Some(((format, samples), pipeline, group));
    }
}

/// Every view's effects resources, made only while a published material blooms or outlines.
#[derive(Resource, Default)]
pub(crate) struct WorldEffectsViews(HashMap<Entity, EffectsView>);

/// Creates or resizes each view's effects resources while a published material blooms or
/// outlines, and writes the widest bloom radius and outline any of them asks for.
pub(crate) fn prepare(
    gpu: Option<Res<super::WorldMaterialGpu>>,
    views: Query<(
        Entity,
        &ViewDepthStencilTexture,
        &ViewTarget,
        Option<&SceneTarget>,
    )>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut effects: ResMut<WorldEffectsViews>,
) {
    let Some(reach) = gpu.and_then(|gpu| gpu.effects_reach()) else {
        effects.0.clear();
        return;
    };
    let mut live = Vec::new();
    for (entity, depth, target, scene) in &views {
        let size = depth.texture().size();
        let samples = depth.texture().sample_count();
        live.push(entity);
        if effects
            .0
            .get(&entity)
            .is_none_or(|view| view.size != size || view.samples != samples)
        {
            effects
                .0
                .insert(entity, EffectsView::new(&device, size, samples));
        }
        if let Some(view) = effects.0.get_mut(&entity) {
            let (_, format, samples) = scene_attachment(target, scene);
            view.ensure_composite(&device, format, samples);
            queue.write_buffer(
                &view.reach,
                0,
                bytemuck::cast_slice(&[reach[0], reach[1], 0.0, 0.0]),
            );
        }
    }
    effects.0.retain(|entity, _| live.contains(entity));
}

/// What the effects pass reads of a view: its targets, depth, view uniform offset, samples and
/// camera.
type EffectsViewQuery = (
    &'static ViewTarget,
    Option<&'static SceneTarget>,
    &'static ViewDepthStencilTexture,
    &'static bevy::render::view::ViewUniformOffset,
    &'static bevy::render::view::Msaa,
    &'static bevy::render::camera::ExtractedCamera,
);

/// Draws the view's bloomed and outlined targets into its effects targets, then blurs, grows
/// and blends them over the scene, after the translucent pass.
pub(crate) fn draw(
    world: &World,
    query: bevy::render::renderer::ViewQuery<EffectsViewQuery>,
    mut context: RenderContext,
) {
    let entity = query.entity();
    let Some(view) = world
        .get_resource::<WorldEffectsViews>()
        .and_then(|views| views.0.get(&entity))
    else {
        return;
    };
    let (target, scene, depth, offset, msaa, camera) = query.into_inner();
    let colors = [
        Some(view.emission.attachment()),
        Some(view.outline.attachment()),
    ];
    let drew = {
        let mut pass = context.begin_tracked_render_pass(
            bevy::render::render_resource::RenderPassDescriptor {
                label: Some("world material effects"),
                color_attachments: &colors,
                depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            },
        );
        let view_key = (entity, offset.offset, *msaa, camera.hdr);
        let actors = crate::actor_render::world_effects::draw(world, view_key, &mut pass);
        let terrain = crate::chunk::world_effects_draw(world, view_key, &mut pass);
        actors || terrain
    };
    if !drew {
        return;
    }
    let (attachment, format, samples) = scene_attachment(target, scene);
    view.encode_passes(context.command_encoder(), attachment, (format, samples));
}

/// The scene colour attachment the composite blends into, loading what is drawn, with its
/// format and sample count.
fn scene_attachment<'a>(
    target: &'a ViewTarget,
    scene: Option<&'a SceneTarget>,
) -> (wgpu::RenderPassColorAttachment<'a>, TextureFormat, u32) {
    match scene {
        Some(scene) => {
            let mut attachment = scene.color_attachment(target, false);
            attachment.ops.load = wgpu::LoadOp::Load;
            let samples = scene.texture.sample_count();
            let format = scene.read_format();
            (attachment, format, samples)
        }
        None => {
            let mut attachment = target.get_color_attachment();
            attachment.ops.load = wgpu::LoadOp::Load;
            let samples = target
                .sampled_main_texture()
                .map_or(1, |texture| texture.sample_count());
            (attachment, target.main_texture_format(), samples)
        }
    }
}

#[cfg(test)]
#[path = "effects_pass_tests.rs"]
mod tests;
