use bevy::prelude::{Msaa, Mut};

use super::super::super::{ActorPipeline, ActorPipelineKey};
use super::*;

/// A blended additive raster no prewarmed built-in material uses.
fn blended() -> render_api::render_state::RenderState {
    use render_api::render_state::{BlendComponent, BlendFactor, BlendOperation, BlendState};
    let component = BlendComponent {
        source: BlendFactor::DestinationColor,
        destination: BlendFactor::OneMinusSourceColor,
        operation: BlendOperation::Add,
    };
    render_api::render_state::RenderState {
        blend: Some(BlendState {
            color: component,
            alpha: component,
        }),
        ..assets::entity::material::UNAUTHORED_RENDER_STATE
    }
}

const AUTHORED: u32 = assets::EntityRenderMaterialState::AUTHORED;

#[test]
fn unrepresentable_target_state_draws_with_the_unauthored_pipeline_and_counts_once() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        let view = (Msaa::Off, false, false);
        pipeline.prewarm(cache, view.0, view.1, view.2).unwrap();
        // From here no target format blends on this adapter.
        pipeline
            .targets
            .set_features(Arc::new(|_| TextureFormatFeatures {
                allowed_usages: bevy::render::render_resource::TextureUsages::all(),
                flags: TextureFormatFeatureFlags::all() - TextureFormatFeatureFlags::BLENDABLE,
            }));
        let unauthored = pipeline
            .draw_variant(
                view.0,
                view.1,
                view.2,
                assets::EntityRenderMaterial::Default as u32,
                assets::entity::material::UNAUTHORED_RENDER_STATE,
            )
            .unwrap();
        for _ in 0..2 {
            pipeline
                .prewarm_rasters(cache, view, [(AUTHORED, blended())])
                .expect("an unrepresentable material never fails preparation");
        }
        assert_eq!(
            pipeline.draw_variant(view.0, view.1, view.2, AUTHORED, blended()),
            Some(unauthored)
        );
        assert_eq!(pipeline.targets.fallback_count(), 1, "counted once");
    });
}

#[test]
fn a_material_pipeline_that_fails_to_compile_falls_back_without_stalling_readiness() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        let view = (Msaa::Off, false, false);
        pipeline.prewarm(cache, view.0, view.1, view.2).unwrap();
        pipeline
            .prewarm_rasters(cache, view, [(AUTHORED, blended())])
            .unwrap();
        let failed = pipeline
            .draw_variant(view.0, view.1, view.2, AUTHORED, blended())
            .unwrap();
        let unauthored = pipeline
            .request_draw(
                cache,
                ActorPipelineKey {
                    msaa: view.0,
                    hdr: view.1,
                    enhanced: view.2,
                    material: assets::EntityRenderMaterial::Default as u32,
                    raster: assets::entity::material::UNAUTHORED_RENDER_STATE,
                },
            )
            .unwrap();
        assert_ne!(failed, unauthored);
        pipeline.settle_failures(cache, |id| Some(id == failed));
        assert_eq!(
            pipeline.draw_variant(view.0, view.1, view.2, AUTHORED, blended()),
            Some(unauthored)
        );
        assert!(
            pipeline.ready_with(view, |id| id != failed),
            "the failed pipeline no longer gates readiness"
        );
        assert_eq!(pipeline.targets.fallback_count(), 1);
        pipeline.settle_failures(cache, |_| Some(true));
        assert_eq!(
            pipeline.targets.fallback_count(),
            1,
            "settled failures are not recounted"
        );
    });
}

#[test]
fn an_unrepresentable_unauthored_contract_still_requests_its_own_pipelines() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        // No target format supports any sample count, so even the unauthored draw is outside
        // what the targets represent; it is the end of every fallback.
        pipeline
            .targets
            .set_features(Arc::new(|_| TextureFormatFeatures {
                allowed_usages: bevy::render::render_resource::TextureUsages::all(),
                flags: TextureFormatFeatureFlags::empty(),
            }));
        let unauthored = ActorPipelineKey {
            msaa: Msaa::Sample4,
            hdr: false,
            enhanced: false,
            material: assets::EntityRenderMaterial::Default as u32,
            raster: assets::entity::material::UNAUTHORED_RENDER_STATE,
        };
        assert!(pipeline.request_draw(cache, unauthored).is_some());
        assert!(pipeline.request_motion(cache, unauthored).is_some());
        let authored = ActorPipelineKey {
            material: AUTHORED,
            raster: blended(),
            ..unauthored
        };
        assert_eq!(
            pipeline.request_motion(cache, authored),
            pipeline.request_motion(cache, unauthored),
            "an authored motion draw falls back to the unauthored one"
        );
    });
}
