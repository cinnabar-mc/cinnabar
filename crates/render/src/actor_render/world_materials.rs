//! Programmable world materials on actor draws: a fragment shader per material, composed from
//! the actor shader and the guest's library, with its textures, parameters and phase in bind
//! group [`WORLD_MATERIAL_GROUP`]. A bound instance draws as Bedrock, with its Bedrock word,
//! raster and pass, until its material's pipeline is ready in every view, and then with the
//! material's tagged word and the host's raster.

use super::*;
use crate::world_material::{ACTOR_SHADERS, ShaderFamily, WorldVariant, layout};

/// The bind group a world material actor pipeline reads its frame, textures and sampler from.
pub(crate) const WORLD_MATERIAL_GROUP: u32 = 2;
/// The actor shader every world material's fragment module extends.
const ACTOR_SOURCE: &str = include_str!("../actor.wesl");

/// How actor modules compose: the production actor shader with the library appended.
pub(crate) fn family() -> ShaderFamily {
    ShaderFamily {
        namespace: ACTOR_SHADERS,
        group: WORLD_MATERIAL_GROUP,
        build: |library, path| {
            crate::shader_safety::from_actor_wesl(
                &format!("{ACTOR_SOURCE}\n{library}"),
                path,
                render_model::ACTOR_GPU_INSTANCE_WORDS,
                render_model::ACTOR_RIG_VERTEX_WORDS,
            )
        },
    }
}

/// Points the fragment stage at `world`'s actor module and adds its bind group.
fn specialize_world(world: WorldVariant, descriptor: &mut RenderPipelineDescriptor) {
    descriptor.layout.truncate(WORLD_MATERIAL_GROUP as usize);
    descriptor.layout.push(layout(world.textures));
    let fragment = descriptor
        .fragment
        .as_mut()
        .expect("actor pipelines draw colour");
    fragment.shader = world.shader(ACTOR_SHADERS);
    fragment.shader_defs.extend(world.defs());
}

/// A base actor pipeline key drawn with a world material's fragment module.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct WorldActorKey {
    pub(super) base: ActorPipelineKey,
    pub(super) world: WorldVariant,
}

impl WorldActorKey {
    /// The guest decides coverage, so the Bedrock material's alpha test and alpha to coverage
    /// do not apply; the raster stays the host's class raster.
    pub(super) fn new(mut base: ActorPipelineKey, world: WorldVariant) -> Self {
        base.material &= !(assets::EntityRenderMaterialState::ALPHA_TEST
            | render_model::actor::frame::ACTOR_WORLD_MATERIAL_MASK);
        base.raster.alpha_to_coverage = false;
        Self { base, world }
    }
}

impl SpecializerKey for WorldActorKey {
    const IS_CANONICAL: bool = false;
    type Canonical = (ActorPipelineContract, WorldVariant);
}

pub(super) struct WorldActorSpecializer;

impl Specializer<RenderPipeline> for WorldActorSpecializer {
    type Key = WorldActorKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let contract = ActorPipelineSpecializer.specialize(key.base, descriptor)?;
        descriptor.label = Some("world material actor pipeline".into());
        specialize_world(key.world, descriptor);
        Ok((contract, key.world))
    }
}

/// Whether the world material pipeline that `base`'s tagged word draws with has compiled and
/// that material's resources exist.
pub(super) fn ready(
    pipeline: &ActorPipeline,
    cache: &PipelineCache,
    world: &crate::world_material::WorldMaterialGpu,
    base: ActorPipelineKey,
) -> bool {
    render_model::actor::frame::actor_world_material(base.material)
        .and_then(|slot| world.variant(slot))
        .and_then(|variant| pipeline.world_variant(WorldActorKey::new(base, variant)))
        .is_some_and(|id| cache.get_render_pipeline(id).is_some())
}

/// `instances` with each of `draws` whose (tagged word, host raster) is in `ready` switched to
/// that word and raster; every other instance keeps its Bedrock word and raster.
pub(super) fn switched(
    instances: &std::sync::Arc<[ActorGpuInstance]>,
    draws: &[render_model::actor::frame::ActorWorldDraw],
    rasters: &[render_api::render_state::RenderState],
    ready: &WorldReady,
) -> std::sync::Arc<[ActorGpuInstance]> {
    let mut output = std::sync::Arc::clone(instances);
    for draw in draws {
        let Some(&raster) = rasters.get(draw.raster as usize) else {
            continue;
        };
        if !ready.contains(&(draw.material, raster)) {
            continue;
        }
        if let Some(instance) =
            std::sync::Arc::make_mut(&mut output).get_mut(draw.instance as usize)
        {
            instance.material = draw.material;
            instance.raster = draw.raster;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_render::world_material::publication::Blend;

    fn base() -> ActorPipelineKey {
        ActorPipelineKey {
            msaa: Msaa::Off,
            hdr: false,
            enhanced: false,
            material: assets::EntityRenderMaterial::Default as u32
                | assets::EntityRenderMaterialState::AUTHORED
                | assets::EntityRenderMaterialState::ALPHA_TEST
                | (3 << render_model::actor::frame::ACTOR_WORLD_MATERIAL_SHIFT),
            raster: render_scene::world_material::host_raster(
                mod_render::world_material::publication::Depth::Test,
                Blend::Premultiplied,
            ),
        }
    }

    #[test]
    fn world_pipelines_swap_only_the_fragment_module_and_add_their_bind_group() {
        let variant = WorldVariant {
            program: 7,
            textures: 2,
            blend: Blend::Premultiplied,
            depth: mod_render::world_material::publication::Depth::Test,
        };
        let key = WorldActorKey::new(base(), variant);
        assert_eq!(
            key.base.material & assets::EntityRenderMaterialState::ALPHA_TEST,
            0,
            "the guest decides coverage"
        );
        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
        let (_, world) = WorldActorSpecializer
            .specialize(key, &mut descriptor)
            .unwrap();
        assert_eq!(world, variant);
        assert_eq!(descriptor.layout.len(), WORLD_MATERIAL_GROUP as usize + 1);
        assert_eq!(descriptor.layout[2], layout(2));
        let fragment = descriptor.fragment.as_ref().unwrap();
        assert_eq!(fragment.shader, variant.shader(ACTOR_SHADERS));
        assert_eq!(descriptor.vertex.shader, ACTOR_SHADER_HANDLE);
        let enabled = |name: &'static str| {
            fragment
                .shader_defs
                .iter()
                .any(|def| *def == bevy::shader::ShaderDefVal::Bool(name.into(), true))
        };
        assert!(enabled("WORLD_MATERIAL") && enabled("WORLD_MATERIAL_PREMULTIPLY"));
        assert!(!enabled("WORLD_MATERIAL_CUTOUT"));
        // The host's class raster blends; the guest chose no factors.
        let target = fragment.targets[0].as_ref().unwrap();
        assert!(target.blend.is_some());
        assert_eq!(
            descriptor
                .depth_stencil
                .as_ref()
                .unwrap()
                .depth_write_enabled,
            Some(false)
        );
        // Distinct programs never share a pipeline.
        let other = WorldVariant {
            program: 8,
            ..variant
        };
        assert_ne!(other.shader(ACTOR_SHADERS), variant.shader(ACTOR_SHADERS));
    }

    #[test]
    fn a_bound_actor_draws_as_bedrock_until_its_world_pipeline_is_ready() {
        use mod_render::world_material::publication::Depth;
        use render_api::render_state::{BlendState, RenderState};
        use std::sync::Arc;
        // An originally blended actor bound to an opaque-class material.
        let bedrock = RenderState {
            blend: Some(BlendState::ALPHA),
            ..Default::default()
        };
        let host = render_scene::world_material::host_raster(Depth::Test, Blend::Opaque);
        let material = render_model::ActorMaterial {
            state: Some(assets::EntityRenderMaterialState::default()),
            render_state: Some(bedrock),
            world: 1,
            world_raster: Some(host),
            ..Default::default()
        };
        let (world_word, _) = material.world_draw().unwrap();
        let rasters = [bedrock, host];
        let instances: Arc<[ActorGpuInstance]> = Arc::from([ActorGpuInstance {
            material: material.gpu_word(),
            raster: 0,
            ..Default::default()
        }]);
        let draws = [render_model::actor::frame::ActorWorldDraw {
            instance: 0,
            material: world_word,
            raster: 1,
        }];
        let drawn = |instances: &[ActorGpuInstance]| {
            let instance = instances[0];
            (
                instance.material,
                instance.raster,
                crate::actor_render::phase::sorted(
                    instance.material,
                    &rasters[instance.raster as usize],
                ),
            )
        };
        let unready = switched(&instances, &draws, &rasters, &WorldReady::new());
        assert_eq!(
            drawn(&unready),
            (material.gpu_word(), 0, true),
            "while compiling or failed: the Bedrock word, raster and sorted pass"
        );
        let ready = switched(
            &instances,
            &draws,
            &rasters,
            &WorldReady::from([(world_word, host)]),
        );
        assert_eq!(
            drawn(&ready),
            (world_word, 1, false),
            "once ready: the tagged word and the host's opaque raster and pass"
        );
    }
}
