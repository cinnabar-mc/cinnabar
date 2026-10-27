//! The world material effects pass on actors: each span bound to a material with a bloom or an
//! outline draws again, with its pose and coverage, into the view's effects targets.

use super::world_materials::{WORLD_MATERIAL_GROUP, WorldActorKey, WorldActorSpecializer};
use super::*;
use crate::world_material::{WorldMaterialGpu, effects_pass, scene::WorldSceneCopies};

/// A world material actor pipeline in its effects-pass form.
pub(super) struct WorldEffectsActorSpecializer;

impl Specializer<RenderPipeline> for WorldEffectsActorSpecializer {
    type Key = WorldActorKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let canonical = WorldActorSpecializer.specialize(key, descriptor)?;
        // The vertex stage takes the material's module only when it displaces.
        effects_pass::specialize(descriptor, key.world.features.displace);
        Ok(canonical)
    }
}

/// Which view the effects pass draws: its entity, view uniform offset, samples and HDR.
pub(crate) type EffectsView = (Entity, u32, Msaa, bool);

/// Draws every actor span bound to a material with a bloom or an outline into `pass`, whose
/// colour attachments are the view's effects targets; returns whether any drew.
pub(crate) fn draw<'w>(
    world: &'w World,
    (view, offset, msaa, hdr): EffectsView,
    pass: &mut TrackedRenderPass<'w>,
) -> bool {
    let (Some(gpu), Some(pipeline), Some(cache), Some(materials), Some(lightmap)) = (
        world.get_resource::<ActorGpu>(),
        world.get_resource::<ActorPipeline>(),
        world.get_resource::<PipelineCache>(),
        world.get_resource::<WorldMaterialGpu>(),
        world.get_resource::<crate::lighting::LightmapGpu>(),
    ) else {
        return false;
    };
    let scene = world
        .get_resource::<WorldSceneCopies>()
        .and_then(|scenes| scenes.group(view));
    let enhanced = world.get::<render_scene::EnhancedRendering>(view).is_some();
    let mut drew = false;
    let mut bound_page = None;
    for span in &gpu.spans {
        let Some((variant, group)) =
            render_model::actor::frame::actor_world_material(span.material)
                .and_then(|slot| materials.slot(slot))
                .filter(|(variant, _)| variant.effects_pass)
        else {
            continue;
        };
        if variant.features.scene && scene.is_none() {
            continue;
        }
        let key = WorldActorKey::new(
            ActorPipelineKey {
                msaa,
                hdr,
                enhanced,
                material: span.material,
                raster: span.raster,
            },
            variant,
        );
        let Some(effects) = pipeline
            .effects_variant(key)
            .and_then(|id| cache.get_render_pipeline(id))
        else {
            continue;
        };
        let page = if span.page == 0 {
            gpu.bind_group.as_ref()
        } else {
            gpu.artwork
                .page(usize::from(span.page) - 1)
                .and_then(|page| page.bind_group.as_ref())
        };
        let Some(page) = page else {
            continue;
        };
        pass.set_render_pipeline(effects);
        if bound_page != Some(span.page) {
            pass.set_bind_group(0, page, &[offset]);
            bound_page = Some(span.page);
        }
        pass.set_bind_group(1, &lightmap.bind_group, &[]);
        pass.set_bind_group(WORLD_MATERIAL_GROUP as usize, group, &[]);
        if let Some(scene) = scene.filter(|_| variant.features.scene) {
            pass.set_bind_group(WORLD_MATERIAL_GROUP as usize + 1, scene, &[]);
        }
        pass.draw(0..span.vertex_count, span.first..span.first + span.count);
        drew = true;
    }
    drew
}

/// Gathers every bound actor of a material with particles, at its feet, for the particle
/// pass.
pub(crate) fn gather_particles(
    gpu: Option<Res<ActorGpu>>,
    materials: Option<Res<WorldMaterialGpu>>,
    mut targets: ResMut<crate::world_material::particles::ParticleTargets>,
) {
    targets.actors.clear();
    let (Some(gpu), Some(materials)) = (gpu, materials) else {
        return;
    };
    if !materials.has_particles() {
        return;
    }
    for span in &gpu.spans {
        let Some(slot) = render_model::actor::frame::actor_world_material(span.material)
            .filter(|slot| materials.particles(*slot).is_some())
        else {
            continue;
        };
        let instances = gpu
            .instances
            .get(span.first as usize..(span.first + span.count) as usize)
            .unwrap_or_default();
        targets
            .actors
            .entry(slot)
            .or_default()
            .extend(instances.iter().map(|instance| {
                let [x, y, z] = instance.world_from_actor.map(|row| row[3]);
                [x, y, z, 1.0]
            }));
    }
    // One draw per actor, not per layer: the body and its controller layers share a position.
    for origins in targets.actors.values_mut() {
        origins.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        origins.dedup();
    }
}
