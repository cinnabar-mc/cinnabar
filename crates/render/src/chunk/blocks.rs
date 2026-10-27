//! Independent terrain arenas for retained block grids, built without frame-thread meshing.

use crate::chunk::*;
use bevy::render::render_resource::{CachedRenderPipelineId, FrontFace};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use gpu::resource_geometry::PreparedResourceGeometry;
use mod_api::drawing::DrawingHandle;
use mod_render::retained::{Blend, Depth, blocks::BlockMesh};
use render_scene::ChunkTextureBindingIdentity;
use render_scene::{
    ChunkRenderInstance, ChunkTextureAssets, ModRenderScene,
    mod_scene::retained::{DrawingFrame, resolve},
};

pub(crate) mod drawing;
mod prepare;
mod sorting;
#[cfg(test)]
mod tests;
pub(in crate::chunk) mod transparent;
use sorting::{Face, Order};

#[derive(Clone, Copy, Hash, PartialEq, Eq, SpecializerKey)]
struct Key {
    msaa: Msaa,
    hdr: bool,
    clipped: bool,
    always: bool,
    additive: bool,
    mirrored: bool,
}
struct Specialization;
impl Specializer<RenderPipeline> for Specialization {
    type Key = Key;
    /// Preserves native terrain raster state while selecting a private content depth attachment.
    fn specialize(
        &self,
        key: Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Key>, BevyError> {
        pipeline::layouts::ChunkPipelineSpecializer.specialize(
            pipeline::layouts::ChunkPipelineKey {
                msaa: key.msaa,
                hdr: key.hdr,
                enhanced: false,
            },
            descriptor,
        )?;
        if key.mirrored {
            descriptor.primitive.front_face = match descriptor.primitive.front_face {
                FrontFace::Ccw => FrontFace::Cw,
                FrontFace::Cw => FrontFace::Ccw,
            };
        }
        let depth = descriptor.depth_stencil.as_mut().unwrap();
        if key.clipped {
            depth.format = crate::mod_render::retained::clips::FORMAT;
            depth.stencil = crate::mod_render::retained::clips::content_stencil();
        }
        if key.always {
            depth.depth_compare = Some(CompareFunction::Always);
        }
        if key.additive {
            let target = descriptor.fragment.as_mut().unwrap().targets[0]
                .as_mut()
                .unwrap();
            // Blended terrain shaders return straight-alpha colour.
            let transparent = target.blend.is_some();
            target.blend = Some(BlendState {
                color: wgpu::BlendComponent {
                    src_factor: if transparent {
                        wgpu::BlendFactor::SrcAlpha
                    } else {
                        wgpu::BlendFactor::One
                    },
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            });
        }
        Ok(key)
    }
}

#[derive(Resource)]
struct Pipelines {
    variants: [Variants<RenderPipeline, Specialization>; 5],
    supported: bool,
}
impl FromWorld for Pipelines {
    /// Uses the terrain owner's canonical shader descriptors and table layouts.
    fn from_world(world: &mut World) -> Self {
        let mut descriptors = pipeline::layouts::descriptors::build(false);
        let transparent = &mut descriptors[3];
        transparent.vertex.entry_point = Some("vertex_blocks".into());
        transparent.vertex.shader_defs.push("MOD_BLOCKS".into());
        Self {
            variants: descriptors.map(|descriptor| Variants::new(Specialization, descriptor)),
            supported: world.get_resource::<RenderDevice>().is_some_and(|device| {
                device.limits().max_storage_buffers_per_shader_stage
                    >= required_vertex_storage_buffers()
            }),
        }
    }
}

struct Native {
    geometry: Arc<mod_render::retained::Geometry>,
    terrain: PreparedResourceGeometry,
    faces: Arc<[Face]>,
    _charge: mod_render::GpuAllocation<()>,
}
struct Pending {
    geometry: Arc<mod_render::retained::Geometry>,
    task: Task<Option<Native>>,
}

struct Placement {
    uniform: bevy::render::render_resource::UniformBuffer<ViewUniform>,
    matrix: Mat4,
    projection: Mat4,
    viewport: bevy::math::UVec4,
    exposure: f32,
    binding: Option<BindGroup>,
    opaque_binding: Option<BindGroup>,
    identity: Option<(
        ChunkTextureBindingIdentity,
        bevy::render::render_resource::BufferId,
        bevy::render::render_resource::BufferId,
    )>,
    native: Arc<Native>,
    order: Option<Order>,
    order_job: Option<Task<Option<Order>>>,
    sort_camera: Option<[u32; 6]>,
    pipelines: [CachedRenderPipelineId; 5],
    clip: Option<DrawingHandle>,
    always: bool,
    active: bool,
    _charge: mod_render::GpuAllocation<()>,
}

#[derive(Resource, Default)]
pub(in crate::chunk) struct Blocks {
    native: BTreeMap<DrawingHandle, Arc<Native>>,
    pending: BTreeMap<DrawingHandle, Pending>,
    placements: BTreeMap<(Entity, DrawingHandle), Placement>,
    ledgers: HashMap<u64, mod_render::GpuLedger>,
    sort_cursor: Option<(Entity, DrawingHandle)>,
    draws: Vec<transparent::Draw>,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum BlockPass {
    Opaque,
    Overlay,
}

#[derive(Resource)]
struct Installed;

/// Installs block drawing once its retained and native terrain owners are available.
pub(crate) fn install(app: &mut App) {
    if !app.is_plugin_added::<crate::ChunkRenderPlugin>()
        || app
            .sub_app(RenderApp)
            .world()
            .contains_resource::<Installed>()
    {
        return;
    }
    crate::pipeline_warmup::register::<Pipelines>(app);
    let render = app.sub_app_mut(RenderApp);
    render
        .insert_resource(Installed)
        .init_resource::<Blocks>()
        .add_render_command::<Transparent3d, transparent::DrawCommands>()
        .add_systems(RenderStartup, bevy::render::init_gpu_resource::<Pipelines>)
        .add_systems(Render, transparent::queue.in_set(RenderSystems::Queue))
        .add_systems(
            Render,
            prepare::prepare.in_set(RenderSystems::PrepareBindGroups),
        );
    install_graph(render.world_mut());
}

/// Registers native world draws before clipped content and before the shared scene resolve.
#[derive(Resource)]
struct GraphInstalled;

/// Installs native retained draws once after Core3d is ready.
pub(crate) fn install_graph(world: &mut World) {
    if world.contains_resource::<GraphInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(bevy::core_pipeline::Core3d, |_, schedule| {
            schedule.configure_sets((
                BlockPass::Opaque
                    .in_set(bevy::core_pipeline::Core3dSystems::MainPass)
                    .after(crate::scene_target::ScenePass::Opaque)
                    .before(crate::scene_target::ScenePass::Transparent),
                BlockPass::Overlay
                    .in_set(bevy::core_pipeline::Core3dSystems::MainPass)
                    .after(crate::scene_target::ScenePass::Transparent)
                    .before(crate::mod_render::retained::clips::ClipPass)
                    .before(crate::scene_target::ScenePass::Finish),
            ));
            schedule.add_systems((
                drawing::world_draw::<false>.in_set(BlockPass::Opaque),
                drawing::world_draw::<true>.in_set(BlockPass::Overlay),
            ));
        })
        .is_ok();
    if installed {
        world.insert_resource(GraphInstalled);
    }
}

/// Returns a typed asynchronous failure through the grid's existing status operation.
fn fail(geometry: &mod_render::retained::Geometry, detail: &'static str) {
    if let Some(blocks) = geometry.blocks() {
        if blocks.render_error().is_none() {
            bevy::log::warn!(reason = detail, "retained block geometry admission failed");
        }
        blocks.fail_render(mod_render::retained::DrawError {
            code: mod_render::retained::ErrorCode::BudgetExceeded,
            detail,
        });
    }
}

/// Converts immutable mesh streams on a worker, leaving ordinary terrain's public inputs intact.
fn instances(mesh: &BlockMesh) -> Vec<ChunkRenderInstance> {
    mesh.sections
        .iter()
        .map(|section| {
            let origin = section.origin;
            let key = SubChunkKey::new(
                0,
                origin[0] / world::SUB_CHUNK_SIDE as i32,
                origin[1] / world::SUB_CHUNK_SIDE as i32,
                origin[2] / world::SUB_CHUNK_SIDE as i32,
            );
            ChunkRenderInstance {
                key,
                origin,
                cube_quads: Arc::from([]),
                cube_lighting: Arc::from([]),
                cube_layout: default(),
                model_refs: Arc::from([]),
                model_lighting: Arc::from([]),
                model_draw_refs: Arc::from([]),
                transparent_model_draw_refs: Arc::from([]),
                liquid_quads: Arc::from([]),
                liquid_lighting: Arc::from([]),
                has_depth_liquid: false,
                has_transparent_liquid: false,
                depth_liquid_start: None,
                biome: meshing::PackedBiomeRecord::fallback(),
                tint_identity: ChunkBiomeTintIdentity::new(0, 0),
                generation: 1,
                priority: render_scene::ChunkUploadPriority(0.),
                token: None,
                publication_permit: None,
            }
            .with_resource_mesh(
                section.mesh.clone(),
                meshing::PackedBiomeRecord::fallback(),
                ChunkBiomeTintIdentity::new(0, 0),
            )
        })
        .collect()
}

impl crate::pipeline_warmup::PrewarmPipelines for Pipelines {
    /// Compiles the fixed native depth and blend variants before gameplay presents them.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        if !self.supported {
            return Ok(());
        }
        for clipped in [false, true] {
            for always in [false, true] {
                for additive in [false, true] {
                    for mirrored in [false, true] {
                        let key = Key {
                            msaa: view.msaa,
                            hdr: view.hdr,
                            clipped,
                            always,
                            additive,
                            mirrored,
                        };
                        for variants in &mut self.variants {
                            ids.push(variants.specialize(cache, key)?);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
