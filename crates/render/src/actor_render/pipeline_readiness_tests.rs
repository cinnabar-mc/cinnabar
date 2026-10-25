use super::*;
use bevy::prelude::Mut;
use std::collections::HashSet;

#[test]
fn multisampled_views_prepare_performance_actor_materials_before_switching() {
    use bevy::ecs::system::RunSystemOnce;

    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<render_scene::ActorPipelineReadiness>();
    world.run_system_once(prepare_actor_pipelines).unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    for (material, raster) in prewarm_materials() {
        assert!(
            pipeline
                .draw_variant(Msaa::Off, false, false, material, raster)
                .is_some()
        );
        if super::super::motion::eligible(material, &raster) {
            assert!(
                pipeline
                    .motion_variant(ActorPipelineKey {
                        msaa: Msaa::Off,
                        hdr: false,
                        enhanced: false,
                        material,
                        raster,
                    })
                    .is_some()
            );
        }
    }
    let warmed = (pipeline.draw_variants.len(), pipeline.motion_ids.len());
    for mut msaa in world.query::<&mut Msaa>().iter_mut(world) {
        *msaa = Msaa::Off;
    }
    world.run_system_once(prepare_actor_pipelines).unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    assert_eq!(
        (pipeline.draw_variants.len(), pipeline.motion_ids.len()),
        warmed
    );
}

#[test]
fn readiness_polls_each_raster_contract_once_and_observes_reload() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        let view = (Msaa::Off, false, false);
        assert!(!pipeline.ready_with(view, |_| panic!("unrequested view has no pipelines")));
        pipeline.prewarm(cache, view.0, view.1, view.2).unwrap();
        let expected: HashSet<_> = prewarm_materials()
            .flat_map(|(material, raster)| {
                let color = pipeline.draw_variant(view.0, view.1, view.2, material, raster);
                let motion = super::super::motion::eligible(material, &raster)
                    .then(|| {
                        pipeline.motion_variant(ActorPipelineKey {
                            msaa: view.0,
                            hdr: view.1,
                            enhanced: view.2,
                            material,
                            raster,
                        })
                    })
                    .flatten();
                [color, motion].into_iter().flatten()
            })
            .collect();
        let mut probes = HashSet::new();
        let allocated = test_alloc::thread_allocations();
        let mut count = 0;
        assert!(pipeline.ready_with(view, |_| {
            count += 1;
            true
        }));
        assert_eq!(test_alloc::thread_allocations(), allocated);
        assert_eq!(count, expected.len());
        assert!(pipeline.ready_with(view, |id| {
            assert!(probes.insert(id));
            true
        }));
        assert_eq!(probes, expected);
        for missing in expected {
            assert!(!pipeline.ready_with(view, |id| id != missing));
            assert!(pipeline.ready_with(view, |_| true));
        }
        assert!(!pipeline.ready_with((Msaa::Sample4, false, false), |_| true));
    });
}

#[test]
fn authored_rasters_skip_motion_pipelines_without_device_support() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        pipeline.motion_supported = false;
        let view = (Msaa::Off, false, false);
        let (material, raster) = prewarm_materials()
            .find(|(material, raster)| super::super::motion::eligible(*material, raster))
            .expect("a built-in raster is motion eligible");
        pipeline
            .prewarm_rasters(cache, view, [(material, raster)])
            .unwrap();
        let key = ActorPipelineKey {
            msaa: view.0,
            hdr: view.1,
            enhanced: view.2,
            material,
            raster,
        };
        assert!(pipeline.motion_variant(key).is_none());
        let color = pipeline
            .draw_variant(view.0, view.1, view.2, material, raster)
            .unwrap();
        assert!(pipeline.ready_with(view, |id| id == color));
    });
}

#[test]
fn prewarming_compiles_one_pipeline_per_native_contract() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        for msaa in [Msaa::Off, Msaa::Sample4] {
            let view = (msaa, false, false);
            pipeline.prewarm(cache, view.0, view.1, view.2).unwrap();
            let keys: Vec<_> = prewarm_materials()
                .map(|(material, raster)| ActorPipelineKey {
                    msaa,
                    hdr: false,
                    enhanced: false,
                    material,
                    raster,
                })
                .collect();
            let draws: HashSet<_> = keys.iter().map(|key| key.contract()).collect();
            let motion: HashSet<_> = keys
                .iter()
                .filter(|key| super::super::motion::eligible(key.material, &key.raster))
                .map(|key| key.contract())
                .collect();
            let requested = pipeline.prewarmed[&view].len();
            assert!(
                requested <= draws.len() + motion.len(),
                "{requested} pipelines for {} native draw contracts",
                draws.len()
            );
            assert!(draws.len() < keys.len());
            let allocated = test_alloc::thread_allocations();
            pipeline.prewarm(cache, view.0, view.1, view.2).unwrap();
            assert_eq!(
                test_alloc::thread_allocations(),
                allocated,
                "a prewarmed view requests nothing again"
            );
        }
    });
}

#[test]
fn glint_worn_and_dyed_native_draws_find_prewarmed_pipelines() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        let rules = assets::facts::pack::rules();
        let (_, native) = rules
            .generated_material_states()
            .find(|(_, native)| native.alpha_test && !native.blend)
            .expect("the native table has an alpha-tested material");
        let authored = render_model::ActorMaterial {
            state: Some(assets::EntityRenderMaterialState {
                alpha_test: true,
                ..Default::default()
            }),
            render_state: Some(native.definition().render_state()),
            ..Default::default()
        };
        let legacy = render_model::ActorGlint {
            profile: render_api::CombatVisuals::Legacy112,
            ..Default::default()
        };
        let draws = [
            // An enchanted held or worn layer keeps its material under the glint kind.
            render_model::ActorMaterial {
                kind: assets::EntityRenderMaterial::Glint,
                ..authored
            },
            // A dyed leather layer under the 1.12 profile, plain and enchanted.
            render_model::ActorMaterial {
                color_mask: true,
                glint: legacy,
                ..authored
            }
            .legacy_worn(false),
            render_model::ActorMaterial {
                color_mask: true,
                glint: legacy,
                ..authored
            }
            .legacy_worn(true),
            // A worn layer that names no material.
            render_model::ActorMaterial {
                glint: legacy,
                ..Default::default()
            },
        ];
        for msaa in [Msaa::Off, Msaa::Sample4] {
            pipeline.prewarm(cache, msaa, false, false).unwrap();
            for draw in draws {
                assert!(
                    pipeline
                        .draw_variant(msaa, false, false, draw.gpu_word(), draw.raster())
                        .is_some(),
                    "{msaa:?} {draw:?}"
                );
            }
        }
    });
}

#[test]
fn a_sessions_dyed_alpha_tested_layer_is_prewarmed_under_msaa() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<render_scene::ActorPipelineReadiness>();
    // An authored raster outside the native table, published the way the session publishes it.
    let raster = render_api::render_state::RenderState {
        cull: render_api::render_state::CullMode::Front,
        ..assets::entity::material::UNAUTHORED_RENDER_STATE
    };
    let authored = render_model::ActorMaterial {
        state: Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            ..Default::default()
        }),
        render_state: Some(raster),
        ..Default::default()
    };
    world.insert_resource(render_scene::ActorRenderFrame {
        authored_rasters: authored.presentation_rasters().collect(),
        ..Default::default()
    });
    for mut msaa in world.query::<&mut Msaa>().iter_mut(world) {
        *msaa = Msaa::Sample4;
    }
    world.run_system_once(prepare_actor_pipelines).unwrap();
    let dyed = render_model::ActorMaterial {
        color_mask: true,
        glint: render_model::ActorGlint {
            profile: render_api::CombatVisuals::Legacy112,
            ..Default::default()
        },
        ..authored
    };
    let hdr = world
        .query::<&bevy::render::camera::ExtractedCamera>()
        .single(world)
        .unwrap()
        .hdr;
    let pipeline = world.resource::<ActorPipeline>();
    assert!(
        pipeline
            .draw_variant(Msaa::Sample4, hdr, false, dyed.gpu_word(), raster)
            .is_some()
    );
}
