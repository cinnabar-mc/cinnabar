//! Visible mesh masks and independent, reusable depth for their contents.

use super::{gpu::Gpu, materials::Materials, pipeline};
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    prelude::*,
    render::{
        Render, RenderSystems,
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        view::{ViewDepthStencilTexture, ViewTarget},
    },
};
use render_scene::{ModRenderScene, mod_scene::retained::DrawingFrame};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub(super) const SHADER: Handle<Shader> =
    bevy::asset::uuid_handle!("c659be43-e915-4472-bc82-4ebc46174340");
pub(super) const MSAA_SHADER: Handle<Shader> =
    bevy::asset::uuid_handle!("5eedbcb4-78a6-4228-872e-ddc2eeed4f63");

/// Uses per-sample world occlusion on multisampled views.
pub(super) fn source(msaa: bool, source: &str) -> String {
    if msaa {
        source
            .replace("texture_depth_2d", "texture_depth_multisampled_2d")
            .replace(
                "position: vec4<f32>)",
                "position: vec4<f32>, @builtin(sample_index) sample: u32)",
            )
            .replace("position.xy), 0)", "position.xy), i32(sample))")
    } else {
        source.to_owned()
    }
}

pub(crate) const FORMAT: TextureFormat = TextureFormat::Depth24PlusStencil8;
/// Conservative depth/stencil storage per physical sample, including backend padding.
const ATTACHMENT_SAMPLE_BYTES: u64 = 8;

/// Requires the final intersection level without changing any mask coverage.
pub(crate) fn content_stencil() -> StencilState {
    let face = StencilFaceState {
        compare: CompareFunction::Equal,
        ..default()
    };
    StencilState {
        front: face,
        back: face,
        read_mask: 0xff,
        write_mask: 0,
    }
}

/// Samples world depth only while rasterizing the clip; contents never bind it.
fn depth_layout(msaa: bool) -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "clip world depth",
        &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Depth,
                view_dimension: TextureViewDimension::D2,
                multisampled: msaa,
            },
            count: None,
        }],
    )
}

pub(super) struct MaskSpecialization;
impl Specializer<RenderPipeline> for MaskSpecialization {
    type Key = pipeline::Key;
    /// Advances each covered sample once, after checking world occlusion in the fragment.
    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        pipeline::Specialization.specialize(key, descriptor)?;
        descriptor.layout.push(depth_layout(key.msaa.samples() > 1));
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.shader = if key.msaa.samples() > 1 {
            MSAA_SHADER
        } else {
            SHADER
        };
        fragment.entry_point = Some("clip_mask_fragment".into());
        fragment.targets[0].as_mut().unwrap().write_mask = ColorWrites::empty();
        let depth = descriptor.depth_stencil.as_mut().unwrap();
        depth.depth_compare = Some(CompareFunction::Always);
        depth.depth_write_enabled = Some(false);
        let face = StencilFaceState {
            compare: CompareFunction::Equal,
            pass_op: StencilOperation::IncrementClamp,
            ..default()
        };
        depth.stencil = StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: 0xff,
        };
        Ok(key)
    }
}

struct Target {
    texture: Texture,
    view: TextureView,
    world_depth: TextureViewId,
    binding: BindGroup,
    _charge: mod_render::GpuAllocation<()>,
    bytes: u64,
}

#[derive(Resource, Default)]
struct Targets {
    targets: HashMap<(Entity, u64), Target>,
    masks: HashMap<pipeline::Key, CachedRenderPipelineId>,
    contents: HashMap<pipeline::Key, CachedRenderPipelineId>,
    ledgers: HashMap<u64, mod_render::GpuLedger>,
    retiring: HashMap<u64, Arc<AtomicU64>>,
}

/// Defers an affordable replacement while earlier attachment storage is retiring.
fn reserve_attachment(
    ledger: &mod_render::GpuLedger,
    bytes: u64,
    retiring: u64,
) -> Result<Option<mod_render::GpuAllocation<()>>, mod_render::retained::ErrorCode> {
    if let Some(charge) = ledger.allocate(bytes, || Some(())) {
        return Ok(Some(charge));
    }
    if bytes <= ledger.limit()
        && ledger.used().saturating_sub(retiring).saturating_add(bytes) <= ledger.limit()
    {
        Ok(None)
    } else {
        Err(mod_render::retained::ErrorCode::BudgetExceeded)
    }
}

/// Keeps replaced storage charged and identifies bytes that GPU completion will release.
fn retire_target(
    targets: &mut Targets,
    owner: u64,
    target: Target,
    retired: &mut Vec<(Target, Arc<AtomicU64>)>,
) {
    let pending = Arc::clone(targets.retiring.entry(owner).or_default());
    pending.fetch_add(target.bytes, Ordering::AcqRel);
    retired.push((target, pending));
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ClipPass;

/// Installs coverage after world transparency and before post-processing or the hand.
pub(super) fn install(app: &mut SubApp) {
    app.init_resource::<Targets>().add_systems(
        Render,
        prepare
            .in_set(RenderSystems::PrepareBindGroups)
            .after(pipeline::bindings),
    );
    install_graph(app.world_mut());
}

/// Registers the per-view pass when Core3d has been initialized.
#[derive(Resource)]
struct GraphInstalled;

/// Installs the clip stage once after the view schedule exists.
pub(crate) fn install_graph(world: &mut World) {
    if world.contains_resource::<GraphInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                draw.in_set(ClipPass)
                    .in_set(Core3dSystems::MainPass)
                    .after(crate::scene_target::ScenePass::Transparent)
                    .before(crate::scene_target::ScenePass::Finish),
            );
        })
        .is_ok();
    if installed {
        world.insert_resource(GraphInstalled);
    }
}

/// Makes attachment refusals visible to every clip sharing the owner's view allocation.
fn fail_owner(scene: &ModRenderScene, owner: u64, code: mod_render::retained::ErrorCode) {
    for resources in &scene.retained {
        for (id, clip) in &resources.clips {
            if id.owner == owner {
                clip.status.complete(Err(code));
            }
        }
    }
}

/// Reserves one private depth/stencil attachment per owner and view, reused across its clips.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy clip preparation borrows the scene, device and per-view resources"
)]
fn prepare(
    scene: Res<ModRenderScene>,
    gpu: Res<Gpu>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut targets: ResMut<Targets>,
    mut pipeline: ResMut<pipeline::Pipeline>,
    views: Query<(Entity, &ViewDepthStencilTexture)>,
) {
    let mut retired = Vec::new();
    let active = |view: Entity, owner: u64| {
        views.contains(view)
            && scene
                .retained
                .iter()
                .any(|r| r.clips.keys().any(|h| h.owner == owner))
    };
    let removed: Vec<_> = targets
        .targets
        .keys()
        .copied()
        .filter(|&(view, owner)| !active(view, owner))
        .collect();
    for key in removed {
        if let Some(old) = targets.targets.remove(&key) {
            retire_target(&mut targets, key.1, old, &mut retired);
        }
    }
    for (&(entity, owner), view) in &gpu.views {
        let Ok((_, depth)) = views.get(entity) else {
            continue;
        };
        if !view.batches.iter().any(|batch| {
            batch.mask
                && scene
                    .retained
                    .iter()
                    .find_map(|resources| batch.clip.and_then(|id| resources.clips.get(&id)))
                    .is_some_and(|clip| !matches!(clip.status.result(), Some(Err(_))))
        }) {
            continue;
        }
        let key = (entity, owner);
        let size = depth.texture().size();
        let samples = depth.texture().sample_count();
        if targets.targets.get(&key).is_none_or(|t| {
            t.texture.size() != size
                || t.texture.sample_count() != samples
                || t.world_depth != crate::scene_sampling::view_depth(depth).id()
        }) {
            if let Some(old) = targets.targets.remove(&key) {
                retire_target(&mut targets, owner, old, &mut retired);
            }
            let bytes = u64::from(size.width)
                * u64::from(size.height)
                * u64::from(samples)
                * ATTACHMENT_SAMPLE_BYTES;
            let ledger = mod_render::GpuReservation::ledger_for(&scene.gpu_reservations, owner)
                .cloned()
                .unwrap_or_else(|| {
                    targets
                        .ledgers
                        .entry(owner)
                        .or_insert_with(|| {
                            mod_render::GpuLedger::new(mod_api::drawing::MAX_CLIP_ATTACHMENT_BYTES)
                        })
                        .clone()
                });
            let retiring = targets
                .retiring
                .get(&owner)
                .map_or(0, |bytes| bytes.load(Ordering::Acquire));
            let charge = match reserve_attachment(&ledger, bytes, retiring) {
                Ok(Some(charge)) => charge,
                Ok(None) => continue,
                Err(code) => {
                    fail_owner(&scene, owner, code);
                    continue;
                }
            };
            let Some(texture) =
                crate::gpu_allocation::checked(&device, "clip depth", bytes, || {
                    device.create_texture(&TextureDescriptor {
                        label: Some("clip content depth"),
                        size,
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: TextureDimension::D2,
                        format: FORMAT,
                        usage: TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                })
            else {
                fail_owner(
                    &scene,
                    owner,
                    mod_render::retained::ErrorCode::BudgetExceeded,
                );
                continue;
            };
            let binding = device.create_bind_group(
                "clip world depth",
                &cache.get_bind_group_layout(&depth_layout(samples > 1)),
                &[BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(crate::scene_sampling::view_depth(
                        depth,
                    )),
                }],
            );
            targets.targets.insert(
                key,
                Target {
                    view: texture.create_view(&default()),
                    texture,
                    world_depth: crate::scene_sampling::view_depth(depth).id(),
                    binding,
                    _charge: charge,
                    bytes,
                },
            );
        }
        for batch in view.batches.iter().filter(|b| b.clip.is_some() && !b.mask) {
            if let Ok(id) = pipeline.variants.specialize(&cache, batch.key) {
                targets.contents.insert(batch.key, id);
            }
        }
        for batch in view.batches.iter().filter(|b| b.mask) {
            if let Ok(id) = pipeline.masks.specialize(&cache, batch.key) {
                targets.masks.insert(batch.key, id);
            }
        }
        for batch in view.batches.iter().filter(|b| b.mask) {
            if targets
                .masks
                .get(&batch.key)
                .and_then(|id| cache.get_render_pipeline(*id))
                .is_some()
                && let Some(clip) = scene
                    .retained
                    .iter()
                    .find_map(|resources| batch.clip.and_then(|id| resources.clips.get(&id)))
            {
                clip.status.complete(Ok(()));
            }
        }
    }
    targets.ledgers.retain(|owner, _| {
        scene
            .retained
            .iter()
            .any(|r| r.clips.keys().any(|id| id.owner == *owner))
    });
    targets.retiring.retain(|owner, pending| {
        pending.load(Ordering::Acquire) != 0
            || scene
                .retained
                .iter()
                .any(|resources| resources.clips.keys().any(|id| id.owner == *owner))
    });
    if !retired.is_empty() {
        crate::device_poll::on_frame_complete(&queue, move || {
            for (target, pending) in retired {
                let bytes = target.bytes;
                drop(target);
                pending.fetch_sub(bytes, Ordering::AcqRel);
            }
        });
    }
}

/// Draws a complete mask chain and every content batch sharing its independent depth.
fn draw(
    world: &World,
    query: ViewQuery<(
        &ViewTarget,
        &bevy::render::camera::ExtractedCamera,
        Option<&bevy::camera::MainPassResolutionOverride>,
    )>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let entity = query.entity();
    let (target, camera, resolution) = query.into_inner();
    let Some(color) = crate::scene_target::retained_color_attachment(world, entity, target, false)
    else {
        return Ok(());
    };
    let (Some(gpu), Some(targets), Some(cache), Some(scene), Some(frame), Some(materials)) = (
        world.get_resource::<Gpu>(),
        world.get_resource::<Targets>(),
        world.get_resource::<PipelineCache>(),
        world.get_resource::<ModRenderScene>(),
        world.get_resource::<DrawingFrame>(),
        world.get_resource::<Materials>(),
    ) else {
        return Ok(());
    };
    for resources in &scene.retained {
        if resources.connection != frame.connection || resources.dimension != frame.dimension {
            continue;
        }
        let Some(owner) = resources.geometry.keys().next().map(|h| h.owner) else {
            continue;
        };
        if !resources
            .geometry
            .keys()
            .next()
            .is_some_and(|h| frame.live_owners.contains(&(h.owner, h.generation)))
        {
            continue;
        }
        let (Some(view), Some(private)) = (
            gpu.views.get(&(entity, owner)),
            targets.targets.get(&(entity, owner)),
        ) else {
            continue;
        };
        let Some(viewport) = crate::render_bounds::camera_viewport(
            camera.viewport.as_ref(),
            resolution,
            crate::render_bounds::extent(&private.view),
        ) else {
            continue;
        };
        for &clip in resources.clips.keys() {
            if matches!(resources.clips[&clip].status.result(), Some(Err(_))) {
                continue;
            }
            if !view.batches.iter().any(|b| !b.mask && b.clip == Some(clip))
                && !crate::chunk::blocks::drawing::contains(world, entity, Some(clip), None)
            {
                continue;
            }
            let mut chain = vec![clip];
            while let Some(parent) = resources.clips[chain.last().unwrap()].parent {
                chain.push(parent);
            }
            chain.reverse();
            let mask_batches: Option<Vec<_>> = chain
                .iter()
                .map(|id| view.batches.iter().find(|b| b.mask && b.clip == Some(*id)))
                .collect();
            let Some(mask_batches) = mask_batches else {
                continue;
            };
            if mask_batches.iter().any(|b| {
                targets
                    .masks
                    .get(&b.key)
                    .and_then(|id| cache.get_render_pipeline(*id))
                    .is_none()
            }) {
                continue;
            }
            let mut attachment = color.clone();
            attachment.ops.load = LoadOp::Load;
            let colors = [Some(attachment)];
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("mesh clip contents"),
                color_attachments: &colors,
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &private.view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(0.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: Some(Operations {
                        load: LoadOp::Clear(0),
                        store: StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_camera_viewport(&viewport);
            pass.set_scissor_rect(
                viewport.physical_position.x,
                viewport.physical_position.y,
                viewport.physical_size.x,
                viewport.physical_size.y,
            );
            for (level, batch) in mask_batches.iter().enumerate() {
                let (Some(mesh), Some(binding)) = (
                    gpu.meshes.get(&batch.geometry),
                    view.bindings.get(&batch.geometry),
                ) else {
                    continue;
                };
                pass.set_render_pipeline(
                    cache
                        .get_render_pipeline(targets.masks[&batch.key])
                        .unwrap(),
                );
                pass.set_stencil_reference(level as u32);
                pass.set_bind_group(0, binding, &[]);
                pass.set_bind_group(1, &private.binding, &[]);
                pass.draw(0..mesh.count, batch.instances.clone());
            }
            pass.set_stencil_reference(chain.len() as u32);
            crate::chunk::blocks::drawing::encode(
                world,
                entity,
                Some(clip),
                false,
                None,
                &mut pass,
            );
            for batch in view
                .batches
                .iter()
                .filter(|b| !b.mask && b.clip == Some(clip))
            {
                let Some(pipeline) = (if let Some(material) = batch.material {
                    materials.ready(material, batch.key)
                } else {
                    targets
                        .contents
                        .get(&batch.key)
                        .and_then(|id| cache.get_render_pipeline(*id))
                }) else {
                    continue;
                };
                let (Some(mesh), Some(binding)) = (
                    gpu.meshes.get(&batch.geometry),
                    view.bindings.get(&batch.geometry),
                ) else {
                    continue;
                };
                pass.set_render_pipeline(pipeline);
                pass.set_bind_group(0, binding, &[]);
                pass.draw(0..mesh.count, batch.instances.clone());
            }
            drop(pass);
            if crate::chunk::blocks::drawing::contains(world, entity, Some(clip), None) {
                let colors = [crate::scene_target::retained_color_attachment(
                    world,
                    entity,
                    target,
                    !camera.hdr,
                )];
                let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                    label: Some("clipped transparent terrain"),
                    color_attachments: &colors,
                    depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                        view: &private.view,
                        depth_ops: Some(Operations {
                            load: LoadOp::Load,
                            store: StoreOp::Store,
                        }),
                        stencil_ops: Some(Operations {
                            load: LoadOp::Load,
                            store: StoreOp::Store,
                        }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_camera_viewport(&viewport);
                pass.set_scissor_rect(
                    viewport.physical_position.x,
                    viewport.physical_position.y,
                    viewport.physical_size.x,
                    viewport.physical_size.y,
                );
                pass.set_stencil_reference(chain.len() as u32);
                crate::chunk::blocks::drawing::encode(
                    world,
                    entity,
                    Some(clip),
                    true,
                    None,
                    &mut pass,
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
