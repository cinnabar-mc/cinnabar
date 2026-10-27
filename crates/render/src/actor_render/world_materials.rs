//! Programmable world materials on actor draws: a fragment shader per material, composed from
//! the actor shader and the guest's library, with its textures, parameters and phase in bind
//! group [`WORLD_MATERIAL_GROUP`]. Draws whose material is not ready use the base pipeline.

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
            key: 7,
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
        // Distinct materials never share a pipeline.
        let other = WorldVariant { key: 8, ..variant };
        assert_ne!(other.shader(ACTOR_SHADERS), variant.shader(ACTOR_SHADERS));
    }
}
