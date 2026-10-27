//! World material particles (`render_world_material_passes`): each view draws, after the
//! translucent pass, `count` camera-facing pixel squares around every bound target of a
//! material with particles, placed and coloured by its `particle` function within its radius,
//! depth-tested against the scene. A view draws at most [`MAX_FRAME_PARTICLES`] squares; when its
//! targets need more, they take turns, each frame starting where the last one stopped.

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

/// One view's particle resources, kept across frames: its uniform, each slot's targets, this
/// frame's draws (slot, squares, pipeline), and the target the next frame's admission starts at.
struct ViewParticles {
    uniform: Buffer,
    slots: HashMap<u16, SlotTargets>,
    draws: Vec<(u16, u32, CachedRenderPipelineId)>,
    cursor: usize,
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

/// Which of a view's targets draw this frame, given each one's squares in a stable order:
/// starting at `cursor` and wrapping, every target whose squares still fit within
/// [`MAX_FRAME_PARTICLES`] is admitted. Returns the admitted mask and the next frame's cursor:
/// the first target passed over that a frame could hold, or `cursor` when none was. Each frame
/// then starts with a target the last one left out, so every target that fits a frame on its
/// own draws within a bounded number of frames.
fn admit(counts: &[u32], cursor: usize) -> (Vec<bool>, usize) {
    let mut admitted = vec![false; counts.len()];
    if counts.is_empty() {
        return (admitted, 0);
    }
    let start = cursor % counts.len();
    let mut budget = MAX_FRAME_PARTICLES;
    let mut next = None;
    for index in (start..counts.len()).chain(0..start) {
        let count = counts[index];
        if count <= budget {
            admitted[index] = true;
            budget -= count;
        } else if next.is_none() && count <= MAX_FRAME_PARTICLES {
            next = Some(index);
        }
    }
    (admitted, next.unwrap_or(start))
}

/// Drops the `pipelines` whose material no slot of `gpu` holds any more.
fn retain_live<V>(pipelines: &mut HashMap<(u128, TextureFormat, u32), V>, gpu: &WorldMaterialGpu) {
    let live = gpu.variant_programs();
    pipelines.retain(|(key, _, _), _| live.contains(key));
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
                cursor: 0,
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
        let mut offered = Vec::new();
        let mut counts = Vec::new();
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
            counts.extend(std::iter::repeat_n(count, origins.len()));
            offered.push((slot, variant, count, origins));
        }
        let (admission, cursor) = admit(&counts, resources.cursor);
        resources.cursor = cursor;
        let mut admission = admission.into_iter();
        for (slot, variant, count, origins) in offered {
            let origins: Vec<[f32; 4]> = origins
                .into_iter()
                .zip(admission.by_ref())
                .filter_map(|(origin, admitted)| admitted.then_some(origin))
                .collect();
            let admitted = origins.len();
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
            queue.write_buffer(&targets.buffer, 0, bytemuck::cast_slice(&origins));
            resources
                .draws
                .push((slot, admitted as u32 * count, pipeline));
        }
    }
    particles
        .views
        .retain(|entity, _| live_views.contains(entity));
    retain_live(&mut particles.pipelines, &gpu);
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

    /// Which targets a mask admits.
    fn indices(admitted: &[bool]) -> Vec<usize> {
        (0..admitted.len())
            .filter(|index| admitted[*index])
            .collect()
    }

    #[test]
    fn targets_past_the_frame_budget_take_their_turn() {
        // Twenty targets of 4096 squares; sixteen fit a frame.
        let counts = [4096; 20];
        let (first, cursor) = admit(&counts, 0);
        assert_eq!(indices(&first), (0..16).collect::<Vec<_>>());
        let (second, cursor) = admit(&counts, cursor);
        assert_eq!(
            indices(&second),
            (0..12).chain(16..20).collect::<Vec<_>>(),
            "the targets left out draw the next frame"
        );
        let (third, _) = admit(&counts, cursor);
        assert_eq!(indices(&third), (0..8).chain(12..20).collect::<Vec<_>>());
        // Smaller targets fill what a larger one leaves; one too large for any frame holds no
        // turn.
        let counts = [MAX_FRAME_PARTICLES, 1, MAX_FRAME_PARTICLES + 1, 1];
        let (first, cursor) = admit(&counts, 0);
        assert_eq!(indices(&first), [0]);
        let (second, cursor) = admit(&counts, cursor);
        assert_eq!(indices(&second), [1, 3]);
        assert_eq!(cursor, 0);
        // Targets that all fit keep their cursor.
        assert_eq!(admit(&[3, 4], 1), (vec![true, true], 1));
        assert_eq!(admit(&[], 5), (Vec::new(), 0));
    }

    /// An entity slot past the terrain mask's 64 bits keeps its particle pipeline, while a
    /// retired material's goes.
    #[test]
    fn particle_pipelines_of_every_entity_slot_stay_cached() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut app, _) = crate::queue_review_support::app();
        let world = app.world_mut();
        world.init_resource::<WorldMaterialGpu>();
        let identifiers: Vec<String> = (0..70).map(|i| format!("example:{i}")).collect();
        let identifiers: Vec<&str> = identifiers.iter().map(String::as_str).collect();
        let mut scene =
            render_scene::world_material::WorldMaterialScene::with_bound_entities(&identifiers);
        // The highest slot draws a program of its own.
        let set = std::sync::Arc::make_mut(&mut scene.set);
        set.materials[69].as_mut().unwrap().material =
            std::sync::Arc::new(mod_render::world_material::WorldMaterial {
                source: "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(1.0), 1.0, vec3<f32>(0.0)); }"
                    .into(),
                textures: 0,
                features: Default::default(),
            });
        world.insert_resource(scene);
        world.run_system_once(super::super::prepare).unwrap();
        let gpu = world.resource::<WorldMaterialGpu>();
        let high = gpu.variant(69).unwrap().program;
        assert_ne!(high, gpu.variant(0).unwrap().program);
        let format = TextureFormat::Rgba8UnormSrgb;
        let mut pipelines = HashMap::from([((high, format, 1), ()), ((u128::MAX, format, 1), ())]);
        retain_live(&mut pipelines, gpu);
        assert!(pipelines.contains_key(&(high, format, 1)));
        assert!(!pipelines.contains_key(&(u128::MAX, format, 1)));
    }
}
