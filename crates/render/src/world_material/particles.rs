//! World material particles (`render_world_material_passes`): each view draws, after the
//! translucent pass, `count` camera-facing pixel squares around every bound target of a
//! material with particles, placed and coloured by its `particle` function within its radius,
//! depth-tested against the scene. A view draws at most [`MAX_FRAME_PARTICLES`] squares; the
//! targets past that wait for a later frame.

use bevy::prelude::{Entity, Query, Res, ResMut, Resource, World};
use bevy::render::{
    render_resource::{
        BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
        Buffer, BufferBindingType, BufferDescriptor, BufferSize, BufferUsages,
        CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction, DepthStencilState,
        FragmentState, MultisampleState, PipelineCache, PrimitiveState, PrimitiveTopology,
        RenderPipelineDescriptor, ShaderStages, StoreOp, TextureFormat, VertexState,
    },
    renderer::{RenderContext, RenderDevice, RenderQueue},
    view::{ExtractedView, ViewDepthStencilTexture, ViewTarget},
};
use std::collections::{BTreeMap, HashMap};

use super::{ShaderFamily, WorldMaterialGpu, WorldVariant};
use crate::scene_target::SceneTarget;

/// Provisional: the most particle squares a view draws a frame.
pub(crate) const MAX_FRAME_PARTICLES: u32 = 1 << 16;
/// The renderer namespace of particle modules.
pub(crate) const PARTICLE_SHADERS: u64 = u64::from_le_bytes(*b"wmpartc\0");
/// The bind group the particle module's material library takes.
const MATERIAL_GROUP: u32 = 1;
/// The host's particle stage.
const PARTICLE_SOURCE: &str = include_str!("particles.wgsl");
/// Bytes of the particle view uniform: the clip matrix and the camera's right and up.
const VIEW_BYTES: u64 = 96;

/// How particle modules compose: the material library in group 1, then the particle stage.
pub(crate) fn family() -> ShaderFamily {
    ShaderFamily {
        namespace: PARTICLE_SHADERS,
        group: MATERIAL_GROUP,
        build: |library, path| {
            crate::shader_safety::from_wgsl(format!("{library}\n{PARTICLE_SOURCE}"), path)
        },
    }
}

/// The particle stage's own group: the view uniform and the targets' positions.
fn view_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "world material particle view",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(VIEW_BYTES),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(16),
                },
                count: None,
            },
        ],
    )
}

/// The particle pipeline of `variant` for a scene colour of `format` with `samples`: solid
/// emissive squares, depth-tested against the scene without writing it.
fn descriptor(
    variant: WorldVariant,
    format: TextureFormat,
    samples: u32,
) -> RenderPipelineDescriptor {
    let shader = variant.shader(PARTICLE_SHADERS);
    let mut layout = vec![view_layout(), variant.layout()];
    if variant.features.scene {
        layout.push(super::scene::scene_layout());
    }
    RenderPipelineDescriptor {
        label: Some("world material particles".into()),
        layout,
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("particle_vertex".into()),
            ..Default::default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("particle_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..Default::default()
        }),
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilState {
            format: bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: MultisampleState {
            count: samples,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Bound blocks' positions per view and material slot, which terrain gathers from the sections
/// it queued, and bound actors' positions per slot.
#[derive(Resource, Default)]
pub(crate) struct ParticleTargets {
    pub(crate) blocks: HashMap<Entity, BTreeMap<u16, Vec<[f32; 4]>>>,
    pub(crate) actors: BTreeMap<u16, Vec<[f32; 4]>>,
}

/// One slot's targets in a view: their buffer, its capacity in targets, and its group.
struct SlotTargets {
    buffer: Buffer,
    capacity: usize,
    group: BindGroup,
}

/// One view's particle resources, kept across frames: its uniform, each slot's targets, and
/// this frame's draws (slot, squares, pipeline).
struct ViewParticles {
    uniform: Buffer,
    slots: HashMap<u16, SlotTargets>,
    draws: Vec<(u16, u32, CachedRenderPipelineId)>,
}

/// Every view's particle draws and the pipelines they use.
#[derive(Resource, Default)]
pub(crate) struct WorldParticles {
    views: HashMap<Entity, ViewParticles>,
    pipelines: HashMap<(u128, TextureFormat, u32), CachedRenderPipelineId>,
}

/// The format and samples of the colour attachment particles draw into.
fn scene_format(target: &ViewTarget, scene: Option<&SceneTarget>) -> (TextureFormat, u32) {
    match scene {
        Some(scene) => (scene.read_format(), scene.texture.sample_count()),
        None => (
            target.main_texture_format(),
            target
                .sampled_main_texture()
                .map_or(1, |texture| texture.sample_count()),
        ),
    }
}

/// The squares `targets` of a material drawing `count` around each may draw within `budget`,
/// and how many targets that admits.
pub(crate) fn admitted(targets: usize, count: u32, budget: &mut u32) -> usize {
    if count == 0 {
        return 0;
    }
    let fits = (*budget / count) as usize;
    let admitted = targets.min(fits);
    *budget -= admitted as u32 * count;
    admitted
}

/// Builds each view's particle draws from the gathered targets, within its budget, and
/// requests the pipelines they need.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    gpu: Option<Res<WorldMaterialGpu>>,
    targets: Res<ParticleTargets>,
    views: Query<(Entity, &ExtractedView, &ViewTarget, Option<&SceneTarget>)>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut particles: ResMut<WorldParticles>,
) {
    let particles = &mut *particles;
    let Some(gpu) = gpu.filter(|gpu| gpu.has_particles()) else {
        particles.views.clear();
        return;
    };
    let mut live_views = Vec::new();
    for (entity, view, target, scene) in &views {
        live_views.push(entity);
        let (format, samples) = scene_format(target, scene);
        let blocks = targets.blocks.get(&entity);
        let slots: std::collections::BTreeSet<u16> = targets
            .actors
            .keys()
            .chain(blocks.into_iter().flat_map(BTreeMap::keys))
            .copied()
            .collect();
        let resources = particles
            .views
            .entry(entity)
            .or_insert_with(|| ViewParticles {
                uniform: device.create_buffer(&BufferDescriptor {
                    label: Some("world material particle view"),
                    size: VIEW_BYTES,
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                slots: HashMap::new(),
                draws: Vec::new(),
            });
        resources.draws.clear();
        resources.slots.retain(|slot, _| slots.contains(slot));
        let world_from_view = view.world_from_view.to_matrix();
        let clip = view
            .clip_from_world
            .unwrap_or_else(|| view.clip_from_view * world_from_view.inverse());
        let mut words = clip.to_cols_array().to_vec();
        words.extend(world_from_view.x_axis.truncate().extend(0.0).to_array());
        words.extend(world_from_view.y_axis.truncate().extend(0.0).to_array());
        queue.write_buffer(&resources.uniform, 0, bytemuck::cast_slice(&words));
        let mut budget = MAX_FRAME_PARTICLES;
        for slot in slots {
            let Some((variant, count)) = gpu.particles(slot) else {
                continue;
            };
            let origins: Vec<[f32; 4]> = targets
                .actors
                .get(&slot)
                .into_iter()
                .chain(blocks.and_then(|blocks| blocks.get(&slot)))
                .flatten()
                .copied()
                .collect();
            let admitted = admitted(origins.len(), count, &mut budget);
            if admitted == 0 {
                continue;
            }
            let pipeline = *particles
                .pipelines
                .entry((variant.program, format, samples))
                .or_insert_with(|| {
                    cache.queue_render_pipeline(descriptor(variant, format, samples))
                });
            let uniform = &resources.uniform;
            let held = resources.slots.entry(slot);
            let targets = match held {
                std::collections::hash_map::Entry::Occupied(entry)
                    if entry.get().capacity >= admitted =>
                {
                    entry.into_mut()
                }
                held => {
                    let capacity = admitted.next_power_of_two();
                    let buffer = device.create_buffer(&BufferDescriptor {
                        label: Some("world material particle targets"),
                        size: capacity as u64 * 16,
                        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let group = device.create_bind_group(
                        Some("world material particle view"),
                        &cache.get_bind_group_layout(&view_layout()),
                        &[
                            BindGroupEntry {
                                binding: 0,
                                resource: uniform.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource: buffer.as_entire_binding(),
                            },
                        ],
                    );
                    let made = SlotTargets {
                        buffer,
                        capacity,
                        group,
                    };
                    match held {
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            entry.insert(made);
                            entry.into_mut()
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => entry.insert(made),
                    }
                }
            };
            queue.write_buffer(
                &targets.buffer,
                0,
                bytemuck::cast_slice(&origins[..admitted]),
            );
            resources
                .draws
                .push((slot, admitted as u32 * count, pipeline));
        }
    }
    particles
        .views
        .retain(|entity, _| live_views.contains(entity));
    let live: Vec<u128> = (0..u16::try_from(u64::BITS).unwrap_or(64))
        .filter_map(|slot| gpu.variant(slot).map(|variant| variant.program))
        .collect();
    particles
        .pipelines
        .retain(|(key, _, _), _| live.contains(key));
}

/// Draws the view's particles into its scene colour after the translucent pass.
pub(crate) fn draw(
    world: &World,
    query: bevy::render::renderer::ViewQuery<(
        &'static ViewTarget,
        Option<&'static SceneTarget>,
        &'static ViewDepthStencilTexture,
    )>,
    mut context: RenderContext,
) {
    let entity = query.entity();
    let (Some(particles), Some(gpu), Some(cache)) = (
        world.get_resource::<WorldParticles>(),
        world.get_resource::<WorldMaterialGpu>(),
        world.get_resource::<PipelineCache>(),
    ) else {
        return;
    };
    let Some(view) = particles
        .views
        .get(&entity)
        .filter(|view| !view.draws.is_empty())
    else {
        return;
    };
    let (target, scene, depth) = query.into_inner();
    let scene_group = world
        .get_resource::<super::scene::WorldSceneCopies>()
        .and_then(|scenes| scenes.group(entity));
    let mut attachment = match scene {
        Some(scene) => scene.color_attachment(target, false),
        None => target.get_color_attachment(),
    };
    attachment.ops.load = wgpu::LoadOp::Load;
    let colors = [Some(attachment)];
    let mut pass =
        context.begin_tracked_render_pass(bevy::render::render_resource::RenderPassDescriptor {
            label: Some("world material particles"),
            color_attachments: &colors,
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    for (slot, instances, pipeline) in &view.draws {
        let (Some(pipeline), Some((variant, material)), Some(targets)) = (
            cache.get_render_pipeline(*pipeline),
            gpu.slot(*slot),
            view.slots.get(slot),
        ) else {
            continue;
        };
        let group = &targets.group;
        if variant.features.scene && scene_group.is_none() {
            continue;
        }
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_bind_group(MATERIAL_GROUP as usize, material, &[]);
        if let Some(scene) = scene_group.filter(|_| variant.features.scene) {
            pass.set_bind_group(MATERIAL_GROUP as usize + 1, scene, &[]);
        }
        pass.draw(0..4, 0..*instances);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_draws_particles_within_its_budget() {
        let mut budget = MAX_FRAME_PARTICLES;
        assert_eq!(admitted(3, 4096, &mut budget), 3);
        assert_eq!(budget, MAX_FRAME_PARTICLES - 3 * 4096);
        // Only whole targets fit; the rest wait.
        assert_eq!(admitted(100, 4096, &mut budget), 13);
        assert_eq!(budget, 0);
        assert_eq!(admitted(1, 1, &mut budget), 0);
        assert_eq!(admitted(5, 0, &mut MAX_FRAME_PARTICLES.clone()), 0);
    }
}
