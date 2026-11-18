use bevy::ecs::system::RunSystemOnce;

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
                    ..Default::default()
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
fn authored_depth_and_channel_overrides_request_draw_and_motion_pipelines() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        for msaa in [Msaa::Off, Msaa::Sample4] {
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
                pipeline
                    .prewarm_rasters(cache, (msaa, false, false), [(material, raster)])
                    .unwrap();
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
fn late_dissolve_color_tests_paired_depth_for_equal_and_default_comparisons() {
    for cull in [false, true] {
        for depth_write in [false, true] {
            let contract = |depth_equal| {
                let state = TestMaterial {
                    depth_equal,
                    cull,
                    depth_write,
                    ..Default::default()
                };
                ActorPipelineKey {
                    msaa: Msaa::Off,
                    hdr: false,
                    enhanced: false,
                    material: state.word(EntityRenderMaterial::DissolveColor)
                        | crate::actor::material::LATE_DISSOLVE_COLOR,
                    raster: state.raster(),
                }
                .contract()
            };
            assert!(contract(true) == contract(false), "{cull} {depth_write}");
        }
    }
}

#[test]
fn unobservable_raster_fields_share_one_contract() {
    // HDR views draw both passes to one format; elsewhere blending also selects the sorted
    // pass's gamma-encoded target, which is observable.
    let contract = |kind, state: TestMaterial| {
        ActorPipelineKey {
            msaa: Msaa::Off,
            hdr: true,
            enhanced: false,
            material: state.word(kind),
            raster: state.raster(),
        }
        .contract()
    };
    let variants = (0..16).map(|bits| TestMaterial {
        blend: bits & 1 != 0,
        additive: bits & 2 != 0,
        disable_alpha_write: bits & 4 != 0,
        disable_color_write: bits & 8 != 0,
        ..Default::default()
    });
    let depth = variants
        .clone()
        .map(|state| contract(EntityRenderMaterial::DissolveDepth, state))
        .collect::<Vec<_>>();
    assert!(
        depth.iter().all(|contract| *contract == depth[0]),
        "a dissolve depth pass writes no colour, so blend and write masks do not matter"
    );
    let unwritten = [false, true].map(|blend| {
        contract(
            EntityRenderMaterial::Default,
            TestMaterial {
                blend,
                disable_color_write: true,
                ..Default::default()
            },
        )
    });
    assert!(
        unwritten[0] == unwritten[1],
        "nothing blends without writes"
    );
    let written = [false, true].map(|blend| {
        contract(
            EntityRenderMaterial::Default,
            TestMaterial {
                blend,
                ..Default::default()
            },
        )
    });
    assert!(
        written[0] != written[1],
        "blending is observable when colour is written"
    );
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
                        pipeline
                            .prewarm_rasters(&cache, (msaa, false, false), [(material, raster)])
                            .unwrap();
                        let id = pipeline
                            .draw_variant(msaa, false, false, material, raster)
                            .expect("coverage variants are requested");
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
                                .expect("motion coverage is requested with its draw");
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
    world.insert_resource(render_scene::ActorRenderFrame {
        authored_rasters: std::sync::Arc::from([(material, raster)]),
        ..Default::default()
    });
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

#[test]
fn a_downgraded_view_requests_the_sessions_authored_rasters_again_before_drawing() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<render_scene::ActorPipelineReadiness>();
    let raster = render_api::render_state::RenderState {
        cull: render_api::render_state::CullMode::Front,
        ..Default::default()
    };
    let material = EntityRenderMaterial::Default as u32;
    world.insert_resource(render_scene::ActorRenderFrame {
        authored_rasters: std::sync::Arc::from([(material, raster)]),
        ..Default::default()
    });
    world
        .run_system_once(super::super::prepare_actor_pipelines)
        .unwrap();
    let (&before, view) = world
        .query::<(&Msaa, &ExtractedCamera)>()
        .single(world)
        .unwrap();
    let hdr = view.hdr;
    // The adapter or settings drop multisampling mid-session: the same session's rasters must be
    // requested for the new view, and its readiness must wait for them.
    let after = if before == Msaa::Off {
        Msaa::Sample4
    } else {
        Msaa::Off
    };
    for mut msaa in world.query::<&mut Msaa>().iter_mut(world) {
        *msaa = after;
    }
    world
        .run_system_once(super::super::prepare_actor_pipelines)
        .unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    let previous = pipeline
        .draw_variant(before, hdr, false, material, raster)
        .expect("the first view keeps its pipeline");
    let current = pipeline
        .draw_variant(after, hdr, false, material, raster)
        .expect("the new view requests the session's raster");
    assert_ne!(previous, current, "sample counts need distinct pipelines");
    assert!(
        !pipeline.ready_with((after, hdr, false), |id| id != current),
        "the new view's readiness waits for the session's raster"
    );
}
