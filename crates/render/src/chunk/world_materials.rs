//! Programmable world materials on bound blocks. Meshing tags a bound block's cube quads and
//! model draws with its material slot; ordinary terrain pipelines skip tagged geometry, and the
//! world material passes draw each visible section's tagged geometry once per material:
//! opaque and cutout classes in the opaque phase, premultiplied and additive classes merged
//! face by face into their section's ordinary translucent order, by the metric ordinary
//! translucent terrain sorts its faces with. A section whose materials are
//! not all ready draws its tagged geometry as Bedrock instead, so a bound block never disappears
//! while its material compiles: cube and opaque model geometry through the fallback pipelines,
//! and translucent model draws in their ordinary translucent order and blending.

use crate::chunk::*;
use crate::world_material::{
    MODEL_SHADERS, ShaderFamily, TERRAIN_SHADERS, WorldMaterialGpu, WorldVariant,
};
use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation};
use mod_render::world_material::publication::{Blend, Depth};
use render_scene::ChunkRenderInstance;

mod draw;
mod effects;
mod interleave;
mod translucent;
pub(in crate::chunk) use draw::{
    DrawWorldMaterialTerrainCommands, WorldTerrainSections, queue_world_material_chunks,
};
pub(crate) use effects::{draw as draw_effects, gather_particles};
pub(in crate::chunk) use translucent::{DrawInterleavedTerrainCommands, InterleavedTerrain};

/// The bind group a world material terrain pipeline reads its frame, textures and sampler from.
pub(crate) const WORLD_MATERIAL_GROUP: u32 = 2;
/// The cube and model terrain shaders a world material's modules extend.
const CHUNK_SOURCE: &str = include_str!("../chunk.wesl");
const MODEL_SOURCE: &str = include_str!("../model.wesl");

/// How cube terrain modules compose: the production cube shader with the library appended.
pub(crate) fn family() -> ShaderFamily {
    ShaderFamily {
        namespace: TERRAIN_SHADERS,
        group: WORLD_MATERIAL_GROUP,
        build: |library, path| {
            crate::shader_safety::from_wesl(
                crate::material_shader::source(&format!("{CHUNK_SOURCE}\n{library}")),
                path,
            )
        },
    }
}

/// How model terrain modules compose: the production model shader with the library appended.
pub(crate) fn model_family() -> ShaderFamily {
    ShaderFamily {
        namespace: MODEL_SHADERS,
        group: WORLD_MATERIAL_GROUP,
        build: |library, path| {
            crate::shader_safety::from_wesl(
                crate::material_shader::source(&format!("{MODEL_SOURCE}\n{library}")),
                path,
            )
        },
    }
}

/// Which terrain geometry a world material pipeline draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::chunk) enum Geometry {
    Cube,
    Model,
}

/// A view's terrain key drawn with a world material.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct WorldTerrainKey {
    pub(in crate::chunk) base: ChunkPipelineKey,
    pub(in crate::chunk) world: WorldVariant,
    pub(in crate::chunk) geometry: Geometry,
    /// Whether this is the effects-pass form, writing the view's effects targets.
    pub(in crate::chunk) effects: bool,
}

impl SpecializerKey for WorldTerrainKey {
    const IS_CANONICAL: bool = true;
    type Canonical = Self;
}

/// Whether a blend class draws in the sorted translucent phase.
pub(in crate::chunk) fn translucent(blend: Blend) -> bool {
    matches!(blend, Blend::Premultiplied | Blend::Additive)
}

/// Specializes the two-sided cube or model pipeline with a material's module and host-owned
/// raster: the class selects blending and depth writes, translucent classes write the
/// translucent pass's target as ordinary translucent terrain does, and `Always` passes
/// regardless of stored depth.
pub(in crate::chunk) struct WorldTerrainSpecializer;

impl Specializer<RenderPipeline> for WorldTerrainSpecializer {
    type Key = WorldTerrainKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let shader = key.world.shader(match key.geometry {
            Geometry::Cube => TERRAIN_SHADERS,
            Geometry::Model => MODEL_SHADERS,
        });
        descriptor.vertex.shader = shader.clone();
        let fragment = descriptor.fragment.as_mut().expect("terrain colour");
        fragment.shader = shader;
        if key.geometry == Geometry::Model {
            // The guest decides coverage, so models use the blend entry without the alpha test.
            fragment.entry_point = Some("fragment_blend".into());
        }
        if translucent(key.world.blend) {
            fragment.shader_defs.push("NATIVE_GAMMA_BLEND".into());
        }
        ChunkPipelineSpecializer.specialize(key.base, descriptor)?;
        if descriptor.layout.len() != WORLD_MATERIAL_GROUP as usize {
            return Err(BevyError::from(
                "world materials do not draw on Enhanced terrain",
            ));
        }
        descriptor.label = Some("world material terrain pipeline".into());
        descriptor.layout.push(key.world.layout());
        if key.world.features.scene {
            descriptor
                .layout
                .push(crate::world_material::scene::scene_layout());
        }
        descriptor.vertex.shader_defs.extend(key.world.defs());
        let fragment = descriptor.fragment.as_mut().expect("terrain colour");
        fragment.shader_defs.extend(key.world.defs());
        if key.effects {
            crate::world_material::effects_pass::specialize(descriptor, true);
            return Ok(key);
        }
        let fragment = descriptor.fragment.as_mut().expect("terrain colour");
        let target = fragment.targets[0].as_mut().expect("terrain colour target");
        let one = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        };
        let blended = match key.world.blend {
            Blend::Opaque | Blend::Cutout => None,
            Blend::Premultiplied => Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            Blend::Additive => Some(BlendState {
                color: one,
                alpha: one,
            }),
        };
        target.blend = blended;
        let depth = descriptor.depth_stencil.as_mut().expect("terrain depth");
        depth.depth_write_enabled = Some(blended.is_none() && key.world.depth == Depth::Test);
        if key.world.depth == Depth::Always {
            depth.depth_compare = Some(CompareFunction::Always);
        }
        Ok(key)
    }
}

/// The fallback pipeline draws every tagged quad of `source`'s geometry as its Bedrock
/// material.
pub(in crate::chunk) fn fallback_descriptor(
    source: &RenderPipelineDescriptor,
) -> RenderPipelineDescriptor {
    let mut descriptor = source.clone();
    descriptor.label = Some("world material fallback terrain pipeline".into());
    descriptor
        .vertex
        .shader_defs
        .push("WORLD_MATERIAL_FALLBACK".into());
    descriptor
}

/// The translucent terrain pipeline for a section whose materials are not ready: `source`'s
/// ordinary translucent pipeline, which also draws the section's tagged translucent model draws
/// as their Bedrock material in its ordinary translucent order.
pub(in crate::chunk) fn bedrock_translucent_descriptor(
    source: &RenderPipelineDescriptor,
) -> RenderPipelineDescriptor {
    let mut descriptor = source.clone();
    descriptor.label = Some("world material fallback translucent terrain pipeline".into());
    descriptor
        .vertex
        .shader_defs
        .push("WORLD_MATERIAL_AS_BEDROCK".into());
    descriptor
}

/// Slot mask of a section: bit `tag - 1` for every world material tag its cube quads and model
/// draws carry.
pub(in crate::chunk) fn slot_mask(instance: &ChunkRenderInstance) -> u64 {
    slot_mask_of(
        &instance.cube_quads,
        instance
            .model_draw_refs
            .iter()
            .chain(instance.transparent_model_draw_refs.iter()),
    )
}

/// [`slot_mask`] over a section's cube quads and model draws.
fn slot_mask_of<'a>(
    quads: &[meshing::PackedQuad],
    draws: impl Iterator<Item = &'a meshing::PackedModelDrawRef>,
) -> u64 {
    let bit = |tag: u8| if tag == 0 { 0 } else { 1_u64 << (tag - 1) };
    let cubes = quads
        .iter()
        .fold(0, |mask, quad| mask | bit(quad.world_material()));
    draws.fold(cubes, |mask, draw| mask | bit(draw.world_material()))
}

#[cfg(test)]
#[path = "world_materials/scale_tests.rs"]
mod scale_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_masks_name_every_tagged_material() {
        let quad = |tag| {
            meshing::PackedQuad::new([0; 3], meshing::Face::PositiveY, 1, 1, 1)
                .with_world_material(tag)
        };
        let draw = |tag| meshing::PackedModelDrawRef::new(0, 1).with_world_material(tag);
        assert_eq!(slot_mask_of(&[quad(0), quad(0)], [draw(0)].iter()), 0);
        assert_eq!(
            slot_mask_of(&[quad(1), quad(3), quad(0), quad(3)], [].iter()),
            0b101
        );
        assert_eq!(
            slot_mask_of(&[quad(1)], [draw(5), draw(0)].iter()),
            0b1_0001
        );
    }

    #[test]
    fn the_bedrock_translucent_pipeline_keeps_the_ordinary_translucent_raster() {
        let source = RenderPipelineDescriptor {
            label: Some("translucent".into()),
            vertex: VertexState {
                shader: CHUNK_SHADER_HANDLE,
                shader_defs: vec!["PACKED_TERRAIN".into()],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: CHUNK_SHADER_HANDLE,
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        let bedrock = bedrock_translucent_descriptor(&source);
        assert!(
            bedrock
                .vertex
                .shader_defs
                .contains(&"WORLD_MATERIAL_AS_BEDROCK".into()),
            "tagged translucent model draws are not hidden"
        );
        assert!(
            bedrock
                .vertex
                .shader_defs
                .contains(&"PACKED_TERRAIN".into())
        );
        assert_eq!(bedrock.vertex.shader, source.vertex.shader);
        assert_eq!(
            bedrock.fragment, source.fragment,
            "Bedrock blending is kept"
        );
        assert_eq!(bedrock.depth_stencil, source.depth_stencil);
        assert_eq!(bedrock.layout, source.layout);
    }

    #[test]
    fn world_terrain_pipelines_own_blend_and_depth_and_swap_both_stages() {
        let base = ChunkPipelineKey {
            msaa: Msaa::Off,
            hdr: false,
            enhanced: false,
        };
        let descriptor = |blend, depth, geometry| {
            let mut descriptor = RenderPipelineDescriptor {
                label: None,
                layout: vec![
                    crate::chunk::pipeline::layouts::opaque_chunk_bind_group_layout(),
                    crate::lighting::layout(),
                ],
                vertex: VertexState {
                    shader: CHUNK_SHADER_HANDLE,
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader: CHUNK_SHADER_HANDLE,
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format: render_scene::SCENE_COLOR_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                depth_stencil: Some(DepthStencilState {
                    format: CORE_3D_DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(CompareFunction::GreaterEqual),
                    stencil: default(),
                    bias: default(),
                }),
                ..default()
            };
            let world = WorldVariant {
                program: 5,
                textures: 1,
                blend,
                depth,
                features: Default::default(),
                effects_pass: false,
            };
            WorldTerrainSpecializer
                .specialize(
                    WorldTerrainKey {
                        base,
                        world,
                        geometry,
                        effects: false,
                    },
                    &mut descriptor,
                )
                .unwrap();
            (descriptor, world)
        };
        let (opaque, world) = descriptor(Blend::Cutout, Depth::Test, Geometry::Cube);
        assert_eq!(opaque.vertex.shader, world.shader(TERRAIN_SHADERS));
        assert_eq!(
            opaque.fragment.as_ref().unwrap().shader,
            world.shader(TERRAIN_SHADERS)
        );
        assert_eq!(opaque.layout.len(), 3);
        assert!(!opaque.multisample.alpha_to_coverage_enabled);
        let depth = opaque.depth_stencil.as_ref().unwrap();
        assert_eq!(depth.depth_write_enabled, Some(true));
        let (additive, _) = descriptor(Blend::Additive, Depth::Test, Geometry::Cube);
        assert!(
            additive
                .fragment
                .as_ref()
                .unwrap()
                .shader_defs
                .contains(&"NATIVE_GAMMA_BLEND".into())
                == crate::chunk::transparent::gamma_pass::admitted(false, Msaa::Off, false),
            "translucent classes write the translucent pass's target"
        );
        assert!(
            additive.fragment.unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend
                .is_some()
        );
        assert_eq!(
            additive.depth_stencil.unwrap().depth_write_enabled,
            Some(false)
        );
        let (model, world) = descriptor(Blend::Opaque, Depth::Test, Geometry::Model);
        let fragment = model.fragment.unwrap();
        assert_eq!(fragment.shader, world.shader(MODEL_SHADERS));
        assert_eq!(fragment.entry_point.as_deref(), Some("fragment_blend"));
        assert!(translucent(Blend::Premultiplied) && !translucent(Blend::Cutout));
        let (through, _) = descriptor(Blend::Opaque, Depth::Always, Geometry::Cube);
        let depth = through.depth_stencil.unwrap();
        assert_eq!(depth.depth_compare, Some(CompareFunction::Always));
        assert_eq!(depth.depth_write_enabled, Some(false));
    }
}
