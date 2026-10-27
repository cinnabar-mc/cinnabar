//! Native materials a pack draw cannot represent. Colour-mask and multitexture artwork is
//! composed into an opaque layer first ([`super::compose`]), so the artwork page alone never
//! keeps an actor native.

use assets::{EntityRenderMaterial, EntityRenderMaterialState};
use render_model::{ActorGpuInstance, BlockEntityFrame};
use render_scene::{ActorRenderFrame, HandRigScene};

/// Keeps materials on their native path when the pack cannot represent their texture or depth state.
pub(crate) fn instance_supported(instance: &ActorGpuInstance) -> bool {
    let kind = instance.material & EntityRenderMaterialState::KIND_MASK;
    instance.mask_layers == [u32::MAX; 2]
        && instance.material & render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG == 0
        && kind != EntityRenderMaterial::DissolveDepth as u32
        && kind != EntityRenderMaterial::DissolveColor as u32
        && !crate::actor::material::state(instance.material).is_some_and(|state| state.emissive)
}

/// Pack actor draws reproduce back-face or no culling, opaque, alpha or one-one additive
/// blending, and the default depth test with depth and every channel written. Other authored
/// raster states stay on the native path.
pub(crate) fn raster_supported(raster: &render_api::render_state::RenderState) -> bool {
    use render_api::render_state::{BlendFactor, BlendState, CompareFunction, CullMode};
    let raster = crate::actor::material::lowered(*raster);
    raster.cull != CullMode::Front
        && raster.depth.compare == Some(CompareFunction::LessEqual)
        && raster.depth.write
        && [
            None,
            Some(BlendState::ALPHA),
            Some(crate::actor::material::additive(BlendFactor::One)),
        ]
        .contains(&raster.blend)
        && raster.write_color
        && raster.write_alpha
        && !raster.alpha_to_coverage
}
/// Checks the current frame before native queues can be suppressed.
pub(crate) fn frame_supported(
    actors: Option<&ActorRenderFrame>,
    hand: Option<&HandRigScene>,
    blocks: Option<&BlockEntityFrame>,
) -> bool {
    let actors_ok = actors.is_none_or(|frame| {
        frame
            .rig
            .instances
            .iter()
            .enumerate()
            .all(|(index, instance)| {
                if !raster_supported(&crate::actor::material::instance_raster(
                    &frame.rig.rasters,
                    instance,
                )) {
                    return false;
                }
                let page = frame.instance_pages.get(index).copied().unwrap_or(0);
                (page == 0 || frame.artwork.pages.get(usize::from(page) - 1).is_some())
                    && instance_supported(instance)
            })
    });
    let hand_ok = hand
        .and_then(|scene| scene.frame.as_ref())
        .is_none_or(|frame| frame.rig.instances.iter().all(instance_supported));
    let blocks_ok = blocks.is_none_or(|frame| {
        frame.additive.is_empty()
            && frame
                .solid
                .iter()
                .chain(frame.overlay.iter())
                .all(|vertex| {
                    vertex.actor_light
                        & render_model::block_entity::mesh::BLOCK_ENTITY_CULL_BACK_FACES
                        == 0
                })
            && frame
                .overlay
                .iter()
                .all(|vertex| vertex.actor_light != u32::MAX)
    });
    actors_ok && hand_ok && blocks_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materials_without_depth_writes_keep_the_native_frame() {
        for depth_write in [false, true] {
            let state = crate::actor::test_material::TestMaterial {
                blend: true,
                depth_write,
                ..Default::default()
            };
            let instance = ActorGpuInstance {
                material: state.word(EntityRenderMaterial::Default),
                ..Default::default()
            };
            assert_eq!(
                instance_supported(&instance) && raster_supported(&state.raster()),
                depth_write
            );
            let mut frame = ActorRenderFrame::default();
            frame.rig.instances = std::sync::Arc::from([instance]);
            frame.rig.rasters = std::sync::Arc::from([state.raster()]);
            assert_eq!(frame_supported(Some(&frame), None, None), depth_write);
        }
    }

    #[test]
    fn geometrically_culled_block_entity_faces_keep_the_native_frame() {
        for overlay in [false, true] {
            let mut frame = BlockEntityFrame::default();
            let vertices = std::sync::Arc::from([render_model::BlockEntityVertex {
                actor_light: render_model::block_entity::mesh::BLOCK_ENTITY_CULL_BACK_FACES,
                ..Default::default()
            }]);
            if overlay {
                frame.overlay = vertices;
            } else {
                frame.solid = vertices;
            }
            assert!(!frame_supported(None, None, Some(&frame)));
        }
    }

    #[test]
    fn emissive_alpha_materials_keep_native_coverage_and_lighting() {
        let instance = ActorGpuInstance {
            material: crate::actor::test_material::TestMaterial {
                emissive: true,
                ..Default::default()
            }
            .word(EntityRenderMaterial::Default),
            ..Default::default()
        };
        assert!(!instance_supported(&instance));
        let mut frame = ActorRenderFrame::default();
        frame.rig.instances = std::sync::Arc::from([instance]);
        assert!(!frame_supported(Some(&frame), None, None));
    }

    #[test]
    fn depth_and_channel_write_overrides_keep_native_rendering() {
        use crate::actor::test_material::TestMaterial;
        for state in [
            TestMaterial {
                depth_equal: true,
                ..Default::default()
            },
            TestMaterial {
                disable_alpha_write: true,
                ..Default::default()
            },
            TestMaterial {
                disable_color_write: true,
                ..Default::default()
            },
        ] {
            assert!(!raster_supported(&state.raster()));
            let mut frame = ActorRenderFrame::default();
            frame.rig.instances = std::sync::Arc::from([ActorGpuInstance {
                material: state.word(EntityRenderMaterial::Default),
                ..Default::default()
            }]);
            frame.rig.rasters = std::sync::Arc::from([state.raster()]);
            assert!(!frame_supported(Some(&frame), None, None));
        }
    }

    #[test]
    fn native_texture_masks_and_paired_depth_materials_keep_native_composition() {
        let mut instance = ActorGpuInstance::default();
        assert!(instance_supported(&instance));
        for material in [
            EntityRenderMaterial::DissolveDepth as u32,
            EntityRenderMaterial::DissolveColor as u32,
            render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG,
        ] {
            instance.material = material;
            assert!(!instance_supported(&instance));
        }
    }

    #[test]
    fn color_mask_and_multitexture_artwork_draws_with_the_pack() {
        let mut frame = ActorRenderFrame {
            instance_pages: std::sync::Arc::from([1]),
            ..Default::default()
        };
        frame.rig.instances = std::sync::Arc::from([ActorGpuInstance::default()]);
        assert!(!frame_supported(Some(&frame), None, None));
        for (color_mask, multitexture) in [(true, false), (false, true)] {
            frame.rig.instances = std::sync::Arc::from([ActorGpuInstance {
                material: if color_mask {
                    EntityRenderMaterialState::COLOR_MASK
                } else {
                    0
                },
                ..Default::default()
            }]);
            let page = render_model::ActorTexturePage {
                width: 1,
                height: 1,
                layers: 1,
                rgba8: std::sync::Arc::from([255; 4]),
                multitexture,
            };
            let mut artwork = render_model::ActorArtworkPages::default();
            artwork.pages = std::sync::Arc::from([page]);
            frame.artwork = std::sync::Arc::new(artwork);
            assert!(frame_supported(Some(&frame), None, None));
        }
    }

    #[test]
    fn beams_and_dragon_rays_stay_on_their_native_material_paths() {
        let mut frame = BlockEntityFrame::default();
        assert!(frame_supported(None, None, Some(&frame)));
        frame.overlay = std::sync::Arc::from([render_model::BlockEntityVertex {
            actor_light: u32::MAX,
            ..Default::default()
        }]);
        assert!(!frame_supported(None, None, Some(&frame)));
        frame.overlay = std::sync::Arc::from([]);
        frame.additive = std::sync::Arc::from([render_model::BlockEntityVertex::default()]);
        assert!(!frame_supported(None, None, Some(&frame)));
    }
}
