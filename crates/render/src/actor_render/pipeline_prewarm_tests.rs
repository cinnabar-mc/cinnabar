use bevy::ecs::system::RunSystemOnce;
use std::collections::HashSet;

use crate::actor::test_material::TestMaterial;
use assets::EntityRenderMaterial;
use bevy::{
    prelude::{Msaa, Mut},
    render::{camera::ExtractedCamera, render_resource::PipelineCache},
};

use super::super::{ActorPipeline, ActorPipelineKey, motion};

#[test]
fn actor_pipeline_prewarm_empty_view_requests_pipelines_without_publishing_pending_work() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<render_scene::ActorPipelineReadiness>();
    world
        .run_system_once(super::super::prepare_actor_pipelines)
        .unwrap();
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedCamera)>()
        .single(world)
        .unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    let cache = world.resource::<PipelineCache>();
    assert!(
        pipeline
            .draw_variant(
                msaa,
                view.hdr,
                false,
                EntityRenderMaterial::Default as u32,
                assets::entity::material::UNAUTHORED_RENDER_STATE
            )
            .is_some()
    );
    assert!(
        pipeline
            .motion_variant(ActorPipelineKey {
                msaa,
                hdr: view.hdr,
                enhanced: false,
                material: EntityRenderMaterial::Default as u32,
                raster: assets::entity::material::UNAUTHORED_RENDER_STATE,
            })
            .is_some()
    );
    assert!(!pipeline.ready(cache, msaa, view.hdr, false));
    assert!(
        !world
            .resource::<render_scene::ActorPipelineReadiness>()
            .is_ready()
    );
}

#[test]
fn generated_material_catalog_is_prewarmed_before_entities_arrive() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedCamera)>()
        .single(world)
        .unwrap();
    let hdr = view.hdr;
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        pipeline
            .prewarm(world.resource::<PipelineCache>(), msaa, hdr, false)
            .unwrap();
        for (name, native) in assets::facts::pack::rules().material_states() {
            let raster = native.definition().render_state();
            let material = render_model::ActorMaterial {
                kind: assets::facts::pack::rules().raster(name),
                state: Some(assets::EntityRenderMaterialState {
                    alpha_test: native.alpha_test && !native.blend,
                    emissive: native.emissive,
                    only_emissive: native.only_emissive,
                    disable_overlay: native.disable_overlay,
                }),
                render_state: Some(raster),
                ..Default::default()
            }
            .gpu_word();
            assert!(
                pipeline
                    .draw_variant(msaa, hdr, false, material, raster)
                    .is_some(),
                "{name}"
            );
        }
    });
}

#[test]
fn authored_depth_and_channel_overrides_have_draw_and_motion_pipelines() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        for msaa in [Msaa::Off, Msaa::Sample4] {
            pipeline.prewarm(cache, msaa, false, false).unwrap();
            for bits in 0..256 {
                let state = TestMaterial {
                    alpha_test: bits & 1 != 0,
                    cull: bits & 2 != 0,
                    blend: bits & 4 != 0,
                    depth_write: bits & 8 != 0,
                    depth_equal: bits & 16 != 0,
                    depth_always: bits & 32 != 0,
                    disable_alpha_write: bits & 64 != 0,
                    disable_color_write: bits & 128 != 0,
                    ..Default::default()
                };
                let material = state.word(EntityRenderMaterial::Default);
                let raster = state.raster();
                assert!(
                    pipeline
                        .draw_variant(msaa, false, false, material, raster)
                        .is_some(),
                    "{bits}"
                );
                if motion::eligible(material, &raster) {
                    assert!(
                        pipeline
                            .motion_variant(ActorPipelineKey {
                                msaa,
                                hdr: false,
                                enhanced: false,
                                material,
                                raster,
                            })
                            .is_some(),
                        "motion {bits}"
                    );
                }
            }
        }
    });
}

#[test]
fn late_dissolve_color_keeps_the_paired_depth_contract_with_overrides() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        pipeline
            .prewarm(world.resource::<PipelineCache>(), Msaa::Off, false, false)
            .unwrap();
        for cull in [false, true] {
            for depth_write in [false, true] {
                let state = TestMaterial {
                    depth_equal: true,
                    cull,
                    depth_write,
                    ..Default::default()
                };
                let material = state.word(EntityRenderMaterial::DissolveColor)
                    | crate::actor::material::LATE_DISSOLVE_COLOR;
                assert!(
                    pipeline
                        .draw_variant(Msaa::Off, false, false, material, state.raster())
                        .is_some()
                );
            }
        }
    });
}

#[test]
fn always_depth_prewarm_covers_blend_and_depth_write_combinations() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        for msaa in [Msaa::Off, Msaa::Sample4] {
            for hdr in [false, true] {
                pipeline.prewarm(cache, msaa, hdr, false).unwrap();
                for kind in [
                    EntityRenderMaterial::Default,
                    EntityRenderMaterial::DissolveDepth,
                    EntityRenderMaterial::DissolveColor,
                ] {
                    for bits in 0..32 {
                        let state = TestMaterial {
                            cull: bits & 1 != 0,
                            blend: bits & 2 != 0,
                            depth_write: bits & 4 != 0,
                            additive: bits & 8 != 0,
                            additive_alpha: bits & 16 != 0,
                            depth_always: true,
                            ..Default::default()
                        };
                        let material = state.word(kind);
                        assert!(
                            pipeline
                                .draw_variant(msaa, hdr, false, material, state.raster())
                                .is_some(),
                            "always-depth material must have a draw pipeline: {kind:?}, {bits}"
                        );
                        if kind == EntityRenderMaterial::DissolveColor {
                            let paired = TestMaterial {
                                depth_always: false,
                                ..state
                            };
                            let word =
                                paired.word(kind) | crate::actor::material::LATE_DISSOLVE_COLOR;
                            assert!(
                                pipeline
                                    .draw_variant(msaa, hdr, false, word, paired.raster())
                                    .is_some(),
                                "paired color must retain its depth contract: {bits}"
                            );
                        }
                    }
                }
            }
        }
    });
}

#[test]
fn actor_pipeline_prewarm_empty_frame_covers_all_authored_raster_states() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<render_scene::ActorRenderFrame>();
    assert!(
        world
            .resource::<render_scene::ActorRenderFrame>()
            .rig
            .instances
            .is_empty()
    );
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedCamera)>()
        .single(world)
        .unwrap();
    let hdr = view.hdr;
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        pipeline
            .prewarm(cache, msaa, hdr, false)
            .expect("an empty view prepares actor pipelines");
        let mut descriptors = HashSet::new();
        let kinds = [
            EntityRenderMaterial::Default,
            EntityRenderMaterial::Glint,
            EntityRenderMaterial::Dragon,
            EntityRenderMaterial::DissolveDepth,
            EntityRenderMaterial::DissolveColor,
        ];
        for kind in kinds {
            for cull in [false, true] {
                for blend in [false, true] {
                    for depth_write in [false, true] {
                        for alpha_test in [false, true] {
                            let state = TestMaterial {
                                alpha_test,
                                cull,
                                blend,
                                depth_write,
                                ..Default::default()
                            };
                            let material = state.word(kind);
                            let raster = state.raster();
                            if motion::eligible(material, &raster) {
                                let key = ActorPipelineKey {
                                    msaa,
                                    hdr,
                                    enhanced: false,
                                    material,
                                    raster,
                                };
                                assert!(
                                    pipeline.motion_variant(key).is_some(),
                                    "motion must be prewarmed: {kind:?}, {material:#x}"
                                );
                            }
                            descriptors.insert(
                                pipeline.draw_variant(msaa, hdr, false, material, raster).expect(
                                    "authored raster states are requested before an actor exists",
                                ),
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(
            descriptors.len(),
            28,
            "only distinct raster/depth contracts compile"
        );
        for kind in kinds {
            for cull in [false, true] {
                for depth_write in [false, true] {
                    for additive_alpha in [false, true] {
                        let state = TestMaterial {
                            cull,
                            depth_write,
                            blend: true,
                            additive: true,
                            additive_alpha,
                            ..Default::default()
                        };
                        descriptors.insert(
                            pipeline
                                .draw_variant(msaa, hdr, false, state.word(kind), state.raster())
                                .expect("each additive kind is prepared before an actor exists"),
                        );
                    }
                }
            }
        }
        assert_eq!(
            descriptors.len(),
            52,
            "additive source factors add eight contracts per raster kind"
        );
        let allocated = test_alloc::thread_allocations();
        pipeline.prewarm(cache, msaa, hdr, false).unwrap();
        assert_eq!(test_alloc::thread_allocations(), allocated);
    });
}

#[test]
fn actor_pipeline_prewarm_reuses_descriptors_for_shader_only_flags() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedCamera)>()
        .single(world)
        .unwrap();
    let hdr = view.hdr;
    let state = TestMaterial::default();
    let raster = state.raster();
    let normal = state.word(EntityRenderMaterial::Default);
    let dragon = TestMaterial {
        alpha_test: true,
        ..state
    }
    .word(EntityRenderMaterial::Dragon);
    let emissive = TestMaterial {
        emissive: true,
        ..state
    }
    .word(EntityRenderMaterial::Default);
    let without_overlay = TestMaterial {
        disable_overlay: true,
        ..state
    }
    .word(EntityRenderMaterial::Default);
    assert_ne!(normal, without_overlay);
    assert_ne!(
        normal, dragon,
        "shader instances retain their full material words"
    );
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        pipeline.prewarm(cache, msaa, hdr, false).unwrap();
        assert_eq!(
            pipeline
                .draw_variant(msaa, hdr, false, normal, raster)
                .unwrap(),
            pipeline
                .draw_variant(msaa, hdr, false, dragon, raster)
                .unwrap(),
            "identical raster contracts share one compiled pipeline",
        );
        assert_eq!(
            pipeline.draw_variant(msaa, hdr, false, normal, raster),
            pipeline.draw_variant(msaa, hdr, false, emissive, raster),
            "emissive shader flags reuse the raster pipeline"
        );
        assert_eq!(
            pipeline.draw_variant(msaa, hdr, false, normal, raster),
            pipeline.draw_variant(msaa, hdr, false, without_overlay, raster),
            "overlay admission reuses the raster pipeline"
        );
    });
}

#[test]
fn actor_coverage_variants_are_prewarmed_without_changing_blend_emissive_or_dissolve() {
    use super::super::{
        ActorPipelineKey, ActorPipelineSpecializer, actor_bind_group_layout,
        actor_pipeline_descriptor,
    };
    use bevy::render::render_resource::Specializer;

    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    app.world_mut().init_resource::<ActorPipeline>();
    let mut pipeline = app.world_mut().remove_resource::<ActorPipeline>().unwrap();
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        pipeline.prewarm(&cache, msaa, false, false).unwrap();
        for kind in [
            EntityRenderMaterial::Default,
            EntityRenderMaterial::Glint,
            EntityRenderMaterial::Dragon,
            EntityRenderMaterial::DissolveDepth,
            EntityRenderMaterial::DissolveColor,
        ] {
            for alpha_test in [false, true] {
                for blend in [false, true] {
                    for emissive in [false, true] {
                        let state = TestMaterial {
                            alpha_test,
                            blend,
                            emissive,
                            ..Default::default()
                        };
                        let material = state.word(kind);
                        let raster = state.raster();
                        let id = pipeline
                            .draw_variant(msaa, false, false, material, raster)
                            .expect("coverage variants are ready before actors exist");
                        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
                        ActorPipelineSpecializer
                            .specialize(
                                ActorPipelineKey {
                                    msaa,
                                    hdr: false,
                                    enhanced: false,
                                    material,
                                    raster,
                                },
                                &mut descriptor,
                            )
                            .unwrap();
                        let enabled = msaa.samples() > 1
                            && alpha_test
                            && !blend
                            && !emissive
                            && matches!(
                                kind,
                                EntityRenderMaterial::Default | EntityRenderMaterial::Glint
                            );
                        assert_eq!(descriptor.multisample.alpha_to_coverage_enabled, enabled);
                        assert_eq!(
                            descriptor
                                .fragment
                                .as_ref()
                                .unwrap()
                                .shader_defs
                                .contains(&crate::alpha_coverage::SHADER_DEF.into()),
                            enabled
                        );
                        let warmed = crate::queue_review_support::queued_descriptor(&mut cache, id);
                        assert_eq!(warmed.multisample.alpha_to_coverage_enabled, enabled);
                        if motion::eligible(material, &raster) {
                            let motion_id = pipeline
                                .motion_variant(ActorPipelineKey {
                                    msaa,
                                    hdr: false,
                                    enhanced: false,
                                    material,
                                    raster,
                                })
                                .expect("motion coverage is prepared before actors exist");
                            let warmed = crate::queue_review_support::queued_descriptor(
                                &mut cache, motion_id,
                            );
                            assert_eq!(warmed.multisample.alpha_to_coverage_enabled, enabled);
                            let fragment = warmed.fragment.as_ref().unwrap();
                            assert_eq!(
                                fragment.entry_point.as_deref(),
                                Some("actor_motion_fragment")
                            );
                            assert_eq!(
                                fragment.targets[0].as_ref().unwrap().format,
                                motion::MOTION_FORMAT
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_frames_authored_raster_is_requested_and_gates_readiness() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<render_scene::ActorPipelineReadiness>();
    let raster = render_api::render_state::RenderState {
        cull: render_api::render_state::CullMode::Front,
        ..Default::default()
    };
    let mut frame = render_scene::ActorRenderFrame::default();
    frame.rig.instances = std::sync::Arc::from([render_model::ActorGpuInstance {
        material: EntityRenderMaterial::Default as u32,
        raster: 0,
        ..Default::default()
    }]);
    frame.rig.rasters = std::sync::Arc::from([raster]);
    world.insert_resource(frame);
    world
        .run_system_once(super::super::prepare_actor_pipelines)
        .unwrap();
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedCamera)>()
        .single(world)
        .unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    let authored = pipeline
        .draw_variant(
            msaa,
            view.hdr,
            false,
            EntityRenderMaterial::Default as u32,
            raster,
        )
        .expect("the frame's authored raster is requested");
    assert!(
        !pipeline.ready_with((msaa, view.hdr, false), |id| id != authored),
        "readiness waits for the authored pipeline"
    );
}

#[test]
fn a_sessions_authored_rasters_are_requested_before_any_actor_draws_them() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<render_scene::ActorPipelineReadiness>();
    let raster = render_api::render_state::RenderState {
        cull: render_api::render_state::CullMode::Front,
        ..Default::default()
    };
    let material = EntityRenderMaterial::Default as u32;
    let mut frame = render_scene::ActorRenderFrame::default();
    frame.authored_rasters = std::sync::Arc::from([(material, raster)]);
    world.insert_resource(frame);
    world
        .run_system_once(super::super::prepare_actor_pipelines)
        .unwrap();
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedCamera)>()
        .single(world)
        .unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    let authored = pipeline
        .draw_variant(msaa, view.hdr, false, material, raster)
        .expect("the session's raster is requested with no actor drawing it");
    assert!(!pipeline.ready_with((msaa, view.hdr, false), |id| id != authored));
}
