use super::*;
use assets::{EntityRenderMaterial, EntityRenderMaterialState};

#[test]
fn overlay_override_keeps_cpu_gpu_policy_without_authoring_raster_state() {
    for kind in [
        EntityRenderMaterial::Default,
        EntityRenderMaterial::Dragon,
        EntityRenderMaterial::Glint,
    ] {
        for state in [
            None,
            Some(EntityRenderMaterialState {
                alpha_test: true,
                ..Default::default()
            }),
            Some(EntityRenderMaterialState {
                emissive: true,
                disable_overlay: true,
                ..Default::default()
            }),
        ] {
            let original = ActorMaterial {
                kind,
                state,
                color_mask: true,
                glint: crate::ActorGlint {
                    profile: render_api::CombatVisuals::Legacy112,
                    ..Default::default()
                },
                ..Default::default()
            };
            assert!(!original.suppress_overlay);
            assert_eq!(
                original.allows_overlay(),
                state.is_none_or(|state| !state.disable_overlay)
            );
            let suppressed = ActorMaterial {
                suppress_overlay: true,
                ..original
            };
            assert!(!suppressed.allows_overlay());
            assert_eq!(suppressed.state, original.state);
            let original_word = original.gpu_word();
            let suppressed_word = suppressed.gpu_word();
            assert_eq!(
                suppressed_word,
                original_word | EntityRenderMaterialState::DISABLE_OVERLAY
            );
            assert_eq!(
                suppressed_word & EntityRenderMaterialState::AUTHORED != 0,
                state.is_some()
            );
            assert_eq!(
                EntityRenderMaterialState::from_word(suppressed_word),
                state.map(|mut state| {
                    state.disable_overlay = true;
                    state
                })
            );
        }
    }
    for kind in [
        EntityRenderMaterial::DissolveDepth,
        EntityRenderMaterial::DissolveColor,
    ] {
        assert!(
            !ActorMaterial {
                kind,
                ..Default::default()
            }
            .allows_overlay()
        );
    }
}

#[test]
fn authored_cull_and_blend_bits_follow_the_raster_state() {
    use render_api::render_state::{BlendState, CullMode, RenderState};
    for (cull, blend) in [
        (CullMode::None, None),
        (CullMode::Back, None),
        (CullMode::Front, Some(BlendState::ALPHA)),
        (CullMode::Back, Some(BlendState::ALPHA)),
    ] {
        let render_state = Some(RenderState {
            cull,
            blend,
            ..Default::default()
        });
        let authored = ActorMaterial {
            state: Some(EntityRenderMaterialState::default()),
            render_state,
            ..Default::default()
        }
        .gpu_word();
        assert_eq!(
            authored & EntityRenderMaterialState::CULL != 0,
            cull == CullMode::Back
        );
        assert_eq!(
            authored & EntityRenderMaterialState::BLEND != 0,
            blend.is_some()
        );
        let unauthored = ActorMaterial {
            render_state,
            ..Default::default()
        }
        .gpu_word();
        assert_eq!(
            unauthored
                & (EntityRenderMaterialState::AUTHORED
                    | EntityRenderMaterialState::CULL
                    | EntityRenderMaterialState::BLEND),
            0
        );
    }
}
