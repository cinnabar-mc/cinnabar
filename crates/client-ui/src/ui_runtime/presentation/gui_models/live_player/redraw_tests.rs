//! Animated HUD models retain surrounding controls and match a full publication.

use super::*;
use crate::ui_runtime::UiRuntime;
use render_model::{UiRenderInput, UiRenderScene, UiRenderStats, hats};
use std::sync::Arc;

/// A small original HUD exercises the production JSON-UI and model paths without game assets.
fn presentation() -> UiPresentationRuntime {
    let mut files = [
        ("ui/_global_variables.json", "{}"),
        ("ui/_ui_defs.json", r#"{"ui_defs":["ui/hud_screen.json"]}"#),
        ("ui/hud_screen.json", r##"{"namespace":"hud","hud_screen":{"type":"screen","controls":[
            {"doll":{"type":"custom","renderer":"hud_player_renderer","size":[48,56],
                "anchor_from":"top_left","anchor_to":"top_left","offset":[8,8],
                "bindings":[{"binding_name":"#paper_doll_visible","binding_name_override":"#visible"}]}}
        ]}}"##),
    ].map(|(path, text)| assets::UiFile { path: path.into(), bytes: text.as_bytes().into() });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let bytes = assets::encode_ui_catalog(
        [1; 32],
        &[assets::UiAtlasPage {
            width: 1,
            height: 1,
            rgba8: vec![255; 4].into(),
        }],
        &[],
        &[],
        &files,
    )
    .unwrap();
    let mut presentation = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
    )
    .unwrap();
    presentation
        .enable_json_ui_inline(Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()))
        .unwrap();
    presentation
}

/// An original two-bone player with an animated attachment and a portable HUD layout.
fn fixture() -> (UiPresentationRuntime, chunk_pipeline::WorldStream) {
    fixture_with_sprinting(false)
}

/// Adds an authored body animation so sprint samples exercise moving player bones as well as wings.
fn fixture_with_sprinting(sprinting: bool) -> (UiPresentationRuntime, chunk_pipeline::WorldStream) {
    let mut presentation = presentation();
    let animation = br#"{"format_version":"1.8.0","animations":{"animation.test.motion":{"loop":true,"bones":{"head":{"rotation":["query.is_sprinting * math.sin(query.life_time * 360) * 20",0,0]}}}}}"#;
    let catalog = tests::player_catalog_with_animation(br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},
        "bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
        {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#.to_vec(), sprinting.then_some(animation.as_slice()));
    let stream = tests::player_stream(Arc::clone(&catalog), catalog);
    presentation.gui_models.enabled = true;
    presentation.set_player_preview_skin(None, Default::default());
    presentation.hud_frame.player_preview = presentation.player_preview_icon();
    presentation.hud_frame.paper_doll_visible = true;
    presentation.hud_frame.first_person = true;
    presentation.set_preview_cosmetics(&[hats::fixture::bunny(), hats::fixture::dragon_wings()]);
    presentation.menu_artwork_loader.wait();
    (presentation, stream)
}

/// Builds a fixed survival HUD while its equipped wing animation advances.
fn frame(
    presentation: &mut UiPresentationRuntime,
    stream: &chunk_pipeline::WorldStream,
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    seconds: f32,
) -> UiRenderInput {
    presentation.set_cosmetic_motion(&[(
        hats::fixture::dragon_wings(),
        hats::Motion {
            seconds,
            ..Default::default()
        },
    )]);
    presentation.capture_hud_player(Some(stream.authority().local_player_ui()), false);
    presentation
        .build(
            player,
            runtime,
            10_000,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

#[test]
fn animated_hud_cosmetics_reuse_topology_and_match_full_frames() {
    let (mut presentation, stream) = fixture();
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    frame(&mut presentation, &stream, &player, &runtime, 0.0);
    let mut previous = frame(&mut presentation, &stream, &player, &runtime, 0.1);
    let mut scene = UiRenderScene::default();
    let stats = UiRenderStats::default();
    scene.publish(previous.clone(), &stats).unwrap();
    for seconds in [0.2, 0.4, 0.7] {
        let next = frame(&mut presentation, &stream, &player, &runtime, seconds);
        assert_ne!(
            previous.models, next.models,
            "the equipped wings must keep moving"
        );
        assert!(
            Arc::ptr_eq(&previous.indices, &next.indices),
            "animation rebuilt HUD topology"
        );
        assert!(Arc::ptr_eq(&previous.batches, &next.batches));
        scene.publish(next.clone(), &stats).unwrap();
        presentation.last_frame = None;
        let full = frame(&mut presentation, &stream, &player, &runtime, seconds);
        assert_eq!(next.vertices, full.vertices);
        assert_eq!(next.models, full.models);
        assert_eq!(next.indices, full.indices);
        assert_eq!(next.batches, full.batches);
        previous = full;
    }
}

/// Moving the doll must update its bone palette while retaining the static vertex stream.
#[test]
fn animated_paper_doll_retains_vertices_without_cpu_projection() {
    let (mut presentation, stream) = fixture();
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    frame(&mut presentation, &stream, &player, &runtime, 0.0);
    let previous = frame(&mut presentation, &stream, &player, &runtime, 0.1);
    super::super::super::player_preview::geometry::take_body_projection_work();
    let next = frame(&mut presentation, &stream, &player, &runtime, 0.7);
    assert_eq!(
        super::super::super::player_preview::geometry::take_body_projection_work(),
        0,
        "animation must not project model vertices on the CPU"
    );
    assert!(
        Arc::ptr_eq(&previous.vertices, &next.vertices),
        "animation must retain the static vertex stream"
    );
    assert!(Arc::ptr_eq(&previous.indices, &next.indices));
    assert!(Arc::ptr_eq(&previous.batches, &next.batches));
    assert!(Arc::ptr_eq(&previous.textures, &next.textures));
}

/// Sprinting advances the authored body and wing poses while sharing every static draw buffer.
#[test]
fn sprinting_paper_doll_updates_bones_without_rebuilding_geometry() {
    let (mut presentation, mut stream) = fixture_with_sprinting(true);
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    let mut feed = cape_tests::feed();
    feed.sprinting = true;
    feed.velocity[2] = 0.28;
    let mut previous: Option<UiRenderInput> = None;
    for step in 1..5 {
        feed.position[2] += feed.velocity[2];
        stream.sync_local_player_pose(&feed);
        stream.advance_actor_interpolation_ticks(1);
        super::super::super::player_preview::geometry::take_body_projection_work();
        let next = frame(
            &mut presentation,
            &stream,
            &player,
            &runtime,
            step as f32 / 20.,
        );
        assert_eq!(
            super::super::super::player_preview::geometry::take_body_projection_work(),
            0
        );
        if let Some(previous) = previous {
            assert!(next.shares_draw_buffers(&previous));
            assert_ne!(
                next.models[0].bones[1], previous.models[0].bones[1],
                "the authored head must move while sprinting"
            );
        }
        previous = Some(next);
    }
}

/// An equal-sized replacement may reference more bones without invalidating the UI publication.
#[test]
fn paper_doll_replacement_validates_its_vertices_with_the_new_palette() {
    let (mut presentation, two_bones) = fixture();
    presentation.set_preview_cosmetics(&[]);
    let one_bone = tests::player_catalog_geometry(br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},
        "bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]},
        {"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#.to_vec());
    let one_bone = tests::player_stream(Arc::clone(&one_bone), one_bone);
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    let previous = frame(&mut presentation, &one_bone, &player, &runtime, 0.);
    let next = frame(&mut presentation, &two_bones, &player, &runtime, 0.);
    assert_eq!(previous.vertices.len(), next.vertices.len());
    assert_eq!(previous.indices, next.indices);
    assert_eq!(previous.batches, next.batches);
    assert!(next.models[0].bones.len() > previous.models[0].bones.len());
    next.validate().unwrap();
}

/// The palette path must preserve the pixels of the original CPU projection at every sampled pose.
#[test]
fn animated_paper_doll_matches_cpu_reference_pixels() {
    let (mut presentation, stream) = fixture();
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    for seconds in [0.0, 0.2, 0.7, 2.1] {
        let gpu = frame(&mut presentation, &stream, &player, &runtime, seconds);
        assert!(!gpu.models.is_empty());
        presentation.gui_models.hud_cpu_reference = true;
        presentation
            .capture_hud_player_reference(Some(stream.authority().local_player_ui()), false);
        presentation.last_frame = None;
        let cpu = presentation
            .build(
                &player,
                &runtime,
                10_000,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert_eq!(
            super::super::super::forms::snapshot::rasterize(&gpu),
            super::super::super::forms::snapshot::rasterize(&cpu),
            "model pixels changed at {seconds}"
        );
        presentation.gui_models.hud_cpu_reference = false;
        presentation.last_frame = None;
    }
}

/// Changing an already admitted cosmetic pose must use only the retained palette scratch.
#[test]
fn animated_paper_doll_model_preparation_allocates_nothing() {
    let (mut presentation, stream) = fixture();
    let sample = |presentation: &mut UiPresentationRuntime, seconds| {
        presentation.set_cosmetic_motion(&[(
            hats::fixture::dragon_wings(),
            hats::Motion {
                seconds,
                ..Default::default()
            },
        )]);
        presentation.capture_hud_player(Some(stream.authority().local_player_ui()), false);
        presentation
            .hud_player_mesh([None, None], |_, _, _, _| true)
            .unwrap()
    };
    for seconds in [0.0, 0.1, 0.2, 0.3] {
        sample(&mut presentation, seconds);
    }
    let (mesh, allocations) = test_alloc::count(|| sample(&mut presentation, 0.7));
    assert_eq!(
        allocations, 0,
        "animation allocated model preparation storage"
    );
    assert!(mesh.is_skinned());
}

#[test]
fn hud_cosmetic_publication_cost() {
    if std::env::var_os("CINNABAR_HUD_MOTION_BENCH").is_none() {
        eprintln!("missing-fixture: opt-in HUD motion benchmark; set CINNABAR_HUD_MOTION_BENCH=1");
        return;
    }
    for (name, visible, wings, sprinting) in [
        ("hidden", false, false, false),
        ("plain", true, false, false),
        ("wings", true, true, false),
        ("wings-sprinting", true, true, true),
    ] {
        let (mut presentation, mut stream) = fixture_with_sprinting(sprinting);
        let mut feed = cape_tests::feed();
        feed.sprinting = true;
        feed.velocity[2] = 0.28;
        presentation.hud_frame.paper_doll_visible = visible;
        let equipped = [hats::fixture::dragon_wings()];
        presentation.set_preview_cosmetics(if wings { &equipped } else { &[] });
        presentation.menu_artwork_loader.wait();
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        let runtime = UiRuntime::new(1);
        let mut scene = UiRenderScene::default();
        let stats = UiRenderStats::default();
        let mut samples = Vec::with_capacity(500);
        let mut capture = 0.0;
        let mut build = 0.0;
        let mut publish = 0.0;
        let mut allocations = 0;
        let mut uploads = 0;
        for step in 0..700 {
            if sprinting {
                feed.position[2] += feed.velocity[2];
                stream.sync_local_player_pose(&feed);
                stream.advance_actor_interpolation_ticks(1);
            }
            let previous = scene.input.clone();
            let started = std::time::Instant::now();
            let ((captured, built, published), allocated) = test_alloc::count(|| {
                if visible {
                    presentation.set_cosmetic_motion(&[(
                        hats::fixture::dragon_wings(),
                        hats::Motion {
                            seconds: step as f32 / 60.0,
                            ..Default::default()
                        },
                    )]);
                    presentation
                        .capture_hud_player(Some(stream.authority().local_player_ui()), false);
                }
                let captured = started.elapsed().as_secs_f64() * 1000.0;
                let input = presentation
                    .build(
                        &player,
                        &runtime,
                        10_000,
                        [1280, 720],
                        ui::DpiScale::new(1.0).unwrap(),
                    )
                    .unwrap();
                let built = started.elapsed().as_secs_f64() * 1000.0;
                scene.publish(input, &stats).unwrap();
                (captured, built, started.elapsed().as_secs_f64() * 1000.0)
            });
            if step >= 200 {
                if let Some(previous) = previous {
                    let input = scene.input.as_ref().unwrap();
                    if !Arc::ptr_eq(&input.vertices, &previous.vertices) {
                        uploads += input
                            .vertices
                            .iter()
                            .zip(previous.vertices.iter())
                            .filter(|(a, b)| a != b)
                            .count();
                    }
                }
                samples.push(published);
                capture += captured;
                build += built - captured;
                publish += published - built;
                allocations += allocated;
            }
        }
        samples.sort_by(f64::total_cmp);
        let input = scene.input.as_ref().unwrap();
        let draws = input
            .batches
            .iter()
            .filter(|batch| batch.isolated_depth_scope.is_some())
            .count();
        eprintln!(
            "HUD_MOTION {name} median_ms={:.4} p99_ms={:.4} capture_ms={:.4} build_ms={:.4} publish_ms={:.4} allocations={} changed_vertices={} model_draws={} builds={}",
            samples[250],
            samples[495],
            capture / 500.0,
            build / 500.0,
            publish / 500.0,
            allocations / 500,
            uploads / 500,
            draws,
            presentation.tree_builds
        );
        super::super::super::forms::snapshot::write(input, &format!("paper-doll-{name}"));
    }
}

/// The published header is recycled only after older immutable readers release it.
#[test]
fn animated_paper_doll_publication_reuses_header() {
    let (mut presentation, stream) = fixture();
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    let mut scene = UiRenderScene::default();
    let stats = UiRenderStats::default();
    for seconds in [0.0, 0.1, 0.2, 0.3] {
        let reader = scene.input.clone();
        scene
            .publish(
                frame(&mut presentation, &stream, &player, &runtime, seconds),
                &stats,
            )
            .unwrap();
        drop(reader);
    }
    let input = frame(&mut presentation, &stream, &player, &runtime, 0.7);
    let held = scene.input.clone().unwrap();
    let revision = held.revision;
    let weak = Arc::downgrade(&held);
    let (_, allocations) = test_alloc::count(|| scene.publish(input, &stats).unwrap());
    assert_eq!(
        allocations, 0,
        "animation allocated another publication header"
    );
    assert_eq!(held.revision, revision);
    assert!(Arc::ptr_eq(&held, &weak.upgrade().unwrap()));
    assert_ne!(scene.revision, held.revision);
}

/// Builds an original opaque sprite that exercises both held-item placements.
fn held_sprite() -> super::super::super::player_preview::PreviewHeldModel {
    use super::super::super::player_preview::{PreviewHeldModel, PreviewHeldPlacement};
    PreviewHeldModel {
        source: ui::IconRef {
            color_mask: None,
            page: 0,
            uv: [0, 0, 1, 1],
            glint: false,
        },
        vertices: render_model::held_sprite_vertices(1, 1, &[255; 4], [0., 0., 1., 1.])
            .unwrap()
            .into(),
        placements: [PreviewHeldPlacement::Sprite {
            hand_equipped: false,
        }; 2],
        hand_pivots: [[0.; 3]; 2],
    }
}

/// A rejected hand candidate must leave the admitted mesh and its allocations resident.
#[test]
fn hud_held_fallback_reuses_the_admitted_mesh() {
    let (mut presentation, stream) = fixture();
    presentation.capture_hud_player(Some(stream.authority().local_player_ui()), false);
    let held = held_sprite();
    let sample = || {
        presentation
            .hud_player_mesh([Some(held.clone()), Some(held.clone())], |_, mask, _, _| {
                mask == [false; 2]
            })
            .unwrap()
    };
    for _ in 0..4 {
        sample();
    }
    let expected = sample();
    let (actual, allocations) = test_alloc::count(sample);
    assert_eq!(
        allocations, 0,
        "rejected hand candidates rebuilt the retained mesh"
    );
    assert!(Arc::ptr_eq(&actual, &expected));
}

/// The exact batch probe keeps HUD models when surrounding sprites can coalesce.
#[test]
fn hud_model_admission_counts_coalesced_surrounding_controls() {
    let (mut presentation, stream) = fixture();
    presentation.player_preview_view = super::super::super::player_preview::PreviewView::Hud;
    presentation.capture_hud_player(Some(stream.authority().local_player_ui()), false);
    let preview = presentation.player_preview_icon.unwrap();
    let bounds = super::super::super::rect(0., 0., 96., 112.).unwrap();
    let mut nodes = (0..ui::UiLimits::MAX_DRAW_BATCHES)
        .map(|index| {
            ui::UiNode::new(ui::UiNodeId::new(index as u32 + 1), None, bounds).with_visual(
                ui::UiVisual::Solid {
                    texture_page: 0,
                    color: [255; 4],
                },
            )
        })
        .collect::<Vec<_>>();
    nodes.push(
        ui::UiNode::new(ui::UiNodeId::new(nodes.len() as u32 + 1), None, bounds).with_visual(
            ui::UiVisual::Sprite {
                texture_page: preview.page,
                uv: preview.uv,
                color: [255; 4],
            },
        ),
    );
    presentation.apply_gui_models(&mut nodes);
    assert!(matches!(
        nodes.last().unwrap().visual(),
        ui::UiVisual::TintedMesh { .. }
    ));
    let draw = ui::RetainedDraw::build(
        &nodes,
        bounds,
        ui::UiScale::default(),
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
    )
    .unwrap();
    assert!(draw.draw_list().batches.len() <= ui::UiLimits::MAX_DRAW_BATCHES);
}

/// Low-alpha dyed armor must reach the color-mask shader before its alpha becomes opaque.
#[test]
fn hud_dyed_armor_preserves_the_color_mask_cutoff() {
    let (mut presentation, stream) = fixture();
    presentation.capture_hud_player(Some(stream.authority().local_player_ui()), false);
    let texture = super::super::super::player_preview::PreviewTexture {
        rgba: Arc::from([200, 100, 30, 1]),
        width: 1,
        height: 1,
        tint: Some([40, 70, 200]),
    };
    presentation.gui_models.sources.textures.insert(
        super::super::atlas::key([1, 1], &texture.rgba),
        ui::IconRef {
            color_mask: None,
            page: 100,
            uv: [0, 0, 1, 1],
            glint: false,
        },
    );
    presentation.player_preview_gear.armor[0] = Some(texture);
    let mesh = presentation
        .hud_player_mesh([None, None], |_, _, _, _| true)
        .unwrap();
    let armor = mesh
        .batches()
        .iter()
        .find(|batch| batch.texture_page == 100)
        .unwrap();
    assert_eq!(armor.alpha_cutoff, Some(1. / 255.));
    assert!(
        mesh.vertices()[armor.index_range.start as usize..armor.index_range.end as usize]
            .iter()
            .all(|vertex| u32::from(vertex.style_flags) & render_model::UI_STYLE_COLOR_MASK != 0)
    );
}

/// The presentation owner can be held by the application's shared Bevy resource.
#[test]
fn hud_model_cache_keeps_the_presentation_send_and_sync() {
    /// Checks the resource boundary without constructing a thread or renderer.
    fn resource<T: Send + Sync>() {}
    resource::<UiPresentationRuntime>();
}

/// Hurt and fire overlays affect the actor while held-item materials keep their own colors.
#[test]
fn hud_actor_overlay_preserves_held_item_colors() {
    let (mut presentation, stream) = fixture();
    presentation.capture_hud_player_reference(Some(stream.authority().local_player_ui()), false);
    for overlay in [[1., 0., 0., 0.25], [0.8, 0.3, 0., 0.7]] {
        presentation.gui_models.live_player.overlay_color = overlay;
        let held = [Some(held_sprite()), Some(held_sprite())];
        let mesh = presentation
            .hud_player_mesh(held.clone(), |_, _, _, _| true)
            .unwrap();
        let actual = presentation
            .gui_models
            .hud_model
            .lock()
            .unwrap()
            .reference_vertices(&mesh);
        let expected = presentation
            .project_gui_player_mesh(
                &presentation
                    .gui_player_sources(super::super::super::player_preview::PreviewView::Hud)
                    .unwrap(),
                held,
                super::super::super::player_preview::PreviewView::Hud,
            )
            .unwrap();
        assert_eq!(actual.len(), expected.vertices().len());
        assert!(actual.iter().any(|vertex| vertex.overlay_color == overlay));
        assert!(actual.iter().any(|vertex| vertex.overlay_color == [0.; 4]));
        for (actual, expected) in actual.iter().zip(expected.vertices()) {
            assert_eq!(actual.overlay_color, expected.overlay_color);
        }
    }
}

/// Simultaneous emote and HUD dolls admit one shared fallback before preparing their palettes.
#[test]
fn hud_and_emote_dolls_share_admission_near_the_frame_budget() {
    use super::super::super::player_preview::{PreviewHandItem, PreviewView};
    let (mut presentation, stream) = fixture();
    presentation.set_preview_cosmetics(&[]);
    presentation.player_preview_view = PreviewView::Hud;
    presentation.hud_preview_view = PreviewView::Hud;
    presentation.capture_hud_player(Some(stream.authority().local_player_ui()), false);
    let mut held = held_sprite();
    let count = (ui::UiLimits::MAX_UI_VERTICES / 2 / 3 + 1) * 3;
    held.vertices = held
        .vertices
        .iter()
        .copied()
        .cycle()
        .take(count)
        .collect::<Vec<_>>()
        .into();
    let key = assets::ItemVisualKey {
        identifier: "fixture:large-held".into(),
        metadata: 0,
    };
    presentation
        .gui_models
        .sources
        .held
        .entry(key.identifier.clone())
        .or_default()
        .insert(key.metadata, super::super::held::HeldModel::ready(held));
    presentation.player_preview_gear.hands[0] = Some(PreviewHandItem {
        shield_pattern: None,
        grip: None,
        glint: None,
        identifier: key.identifier.into(),
        metadata: 0,
        use_animation: Default::default(),
        charged_projectile: Default::default(),
    });
    let icons = assets::encode_icon_catalog([1; 32], &[], &[]).unwrap();
    presentation.icon_catalog = Some(Arc::new(
        assets::RuntimeIconCatalog::decode(&icons).unwrap(),
    ));
    let icon = presentation.player_preview_icon.unwrap();
    let bounds = super::super::super::rect(0., 0., 96., 112.).unwrap();
    let placeholders = [1, 2].map(|id| {
        ui::UiNode::new(ui::UiNodeId::new(id), None, bounds).with_visual(ui::UiVisual::Sprite {
            texture_page: icon.page,
            uv: icon.uv,
            color: [255; 4],
        })
    });
    let sample = || {
        let mut nodes = placeholders.clone();
        presentation.apply_gui_models_in(
            &mut nodes,
            bounds,
            ui::SafeArea::ZERO,
            ui::TextEffects::default(),
            0..1,
        );
        let meshes = nodes.each_ref().map(|node| match node.visual() {
            ui::UiVisual::TintedMesh { mesh, .. } => Arc::clone(mesh),
            _ => panic!("each live control must retain its model"),
        });
        assert!(
            Arc::ptr_eq(&meshes[0], &meshes[1]),
            "controls admitted different variants"
        );
        assert!(
            meshes[0].vertices().len() < count,
            "optional hands must fit both dolls together"
        );
        meshes
    };
    for _ in 0..4 {
        sample();
    }
    let expected = sample();
    let (actual, allocations) = test_alloc::count(sample);
    assert!(Arc::ptr_eq(&expected[0], &actual[0]));
    assert_eq!(allocations, 0, "shared fallback rebuilt per control");
}
