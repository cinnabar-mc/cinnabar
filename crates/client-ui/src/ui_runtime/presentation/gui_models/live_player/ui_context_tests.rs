//! Original actor and item fixtures exercise live UI rendering without installed carriers.

use super::super::super::player_preview::{
    self, PreviewHeldModel, PreviewHeldPlacement, PreviewView,
};
use super::*;
use std::sync::Arc;
use ui::{IconRef, UiMeshVertex, UiNode, UiNodeId, UiVisual};

/// Compiles distinct world and UI controllers plus an animation-controller-driven item pose.
fn pack() -> Arc<assets::SessionEntityPack> {
    pack_with_alternate(false)
}

/// Optionally places a second controller before the masked body to exercise authored draw order.
fn pack_with_alternate(alternate: bool) -> Arc<assets::SessionEntityPack> {
    pack_with_files(alternate, Vec::new())
}

/// Compiles optional original attachable documents beside the player.
fn pack_with_files(
    alternate: bool,
    extra: Vec<(Box<str>, Vec<u8>)>,
) -> Arc<assets::SessionEntityPack> {
    pack_with_overlay(alternate, extra, false)
}

/// Adds independently colored secondary artwork to an alternate controller.
fn pack_with_overlay(
    alternate: bool,
    extra: Vec<(Box<str>, Vec<u8>)>,
    overlay: bool,
) -> Arc<assets::SessionEntityPack> {
    let mut entity = serde_json::json!({"format_version":"1.10.0","minecraft:client_entity":{"description":{
        "identifier":"minecraft:player","materials":{"default":"entity_alphatest"},
        "textures":{"default":"textures/entity/fixture"},"geometry":{"default":"geometry.fixture"},
        "animations":{"pose":"controller.animation.fixture","ui":"animation.fixture.ui"},
        "scripts":{"animate":["pose"], "pre_animation":["variable.show_item = query.is_in_ui;"]},
        "hide_held_items":"!query.is_in_ui || query.mark_variant == 3",
        "held_item_scale":"query.is_in_ui ? 1.5 : 0.5",
        "render_controllers":[{"controller.render.world":"!query.is_in_ui"},
            {"controller.render.ui":"query.is_in_ui && query.mark_variant != 2"}]
    }}});
    let mut geometry = serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
    "description":{"identifier":"geometry.fixture","texture_width":16,"texture_height":16},
    "bones":[
        {"name":"body","pivot":[0,16,0],"cubes":[{"origin":[-4,8,-2],"size":[8,8,4],"uv":[0,0]}]},
        {"name":"rightArm","parent":"body","pivot":[-6,16,0],"cubes":[{"origin":[-8,6,-2],"size":[4,10,4],"uv":[0,0]}]},
        {"name":"rightSleeve","parent":"rightArm","pivot":[-6,16,0],"cubes":[{"origin":[-8,6,-2],"size":[4,10,4],"inflate":0.25,"uv":[0,0]}]},
        {"name":"rightItem","parent":"rightArm","pivot":[-6,7,1]},
        {"name":"leftItem","parent":"body","pivot":[6,7,1]},
        {"name":"head","parent":"body","pivot":[0,24,0]}
    ]}]});
    let controller = serde_json::json!({"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]});
    let mut ui = controller.clone();
    ui["part_visibility"] = serde_json::json!([{"*":false},
        {"rightArm":"query.mark_variant == 1 && !query.is_item_name_any('slot.weapon.mainhand', 'minecraft:stone')"},
        {"rightSleeve":"query.mark_variant == 1 && !query.is_item_name_any('slot.weapon.mainhand', 'minecraft:stone')"}]);
    let mut render = serde_json::json!({"format_version":"1.8.0","render_controllers":{
        "controller.render.world":controller,"controller.render.ui":ui}});
    let animation = serde_json::json!({"format_version":"1.8.0","animations":{"animation.fixture.ui":{
        "loop":true,"bones":{"body":{"rotation":[0,0,30]},
            "rightItem":{"position":["query.life_time * 4",1,0],"rotation":[20,0,45]},
            "leftItem":{"position":[-2,0,1],"rotation":[0,35,0]}}
    }}});
    let state = serde_json::json!({"format_version":"1.10.0","animation_controllers":{"controller.animation.fixture":{
        "initial_state":"default","states":{
            "default":{"transitions":[{"ui":"query.is_in_ui"}]},
            "ui":{"animations":["ui"],"transitions":[{"default":"!query.is_in_ui"}]}
        }
    }}});
    if alternate {
        entity["minecraft:client_entity"]["description"]["geometry"]["alternate"] =
            serde_json::json!("geometry.alternate");
        entity["minecraft:client_entity"]["description"]["render_controllers"]
            .as_array_mut()
            .unwrap()
            .insert(
                0,
                serde_json::json!({"controller.render.extra":"query.is_in_ui"}),
            );
        geometry["minecraft:geometry"].as_array_mut().unwrap().push(serde_json::json!({
            "description":{"identifier":"geometry.alternate","texture_width":16,"texture_height":16},
            "bones":[{"name":"body","pivot":[0,0,0],"cubes":[{"origin":[4,10,0],"size":[2,3,4],"uv":[0,0]}]},
                {"name":"trim","parent":"body","pivot":[0,0,0],"cubes":[{"origin":[10,10,0],"size":[2,3,4],"uv":[0,0]}]}]
        }));
        render["render_controllers"]["controller.render.extra"] = serde_json::json!({
            "geometry":"Geometry.alternate","materials":[{"*":"Material.default"}],"textures":["Texture.default"],
            "part_visibility":[{"trim":false}]
        });
    }
    if overlay {
        entity["minecraft:client_entity"]["description"]["textures"]["overlay"] =
            serde_json::json!("textures/entity/overlay");
        render["render_controllers"]["controller.render.extra"]["textures"] =
            serde_json::json!(["Texture.default", "Texture.overlay"]);
    }
    let mut files: Vec<(Box<str>, Vec<u8>)> = [
        ("entity/player.json", entity),
        ("models/entity/fixture.geo.json", geometry),
        ("render_controllers/fixture.json", render),
        ("animations/fixture.json", animation),
        ("animation_controllers/fixture.json", state),
    ]
    .into_iter()
    .map(|(path, value)| (path.into(), serde_json::to_vec(&value).unwrap()))
    .collect();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 200, 150, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    files.push(("textures/entity/fixture.png".into(), png.into_inner()));
    if overlay {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(16, 16, image::Rgba([220, 60, 30, 128]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        files.push(("textures/entity/overlay.png".into(), png.into_inner()));
    }
    files.extend(extra);
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    Arc::new(assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment: Some(Arc::new(
            assets::RuntimeEquipmentCatalog::from_parts(
                compiled.identity,
                compiled.equipment_bindings,
                compiled.equipment_textures,
            )
            .unwrap(),
        )),
    })
}

/// Prepares the normal preview equipment lookup with an original cube and its own texture page.
fn presentation(pack: Arc<assets::SessionEntityPack>) -> UiPresentationRuntime {
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
    )
    .unwrap();
    let mut ui = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    ui.gui_models.enabled = true;
    ui.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    ui.set_preview_player_layers(Some(pack));
    let held = PreviewHeldModel {
        alpha_mode: render_model::HandItemAlphaMode::Cutout,
        source: IconRef {
            page: 0,
            uv: [0, 0, 1, 1],
            color_mask: None,
            glint: false,
        },
        vertices: render_model::textured_cube_vertices([[0., 0., 1., 1.]; 6]).into(),
        placements: [PreviewHeldPlacement::Block; 2],
        // The captured actor's item bone must replace these standalone preview pivots.
        hand_pivots: [[80., 90., 100.]; 2],
    };
    ui.gui_models.sources.held.insert(
        "minecraft:stone".into(),
        [(0, super::super::held::HeldModel::ready(held))].into(),
    );
    ui.set_player_preview_gear([None; 4], Some(("minecraft:stone", 0)));
    ui
}

/// Publishes the same main-hand identity consumed by actor queries and inventory presentation.
fn equip(stream: &mut chunk_pipeline::WorldStream, item: Option<&str>) {
    stream.sync_local_player_pose(&client_world::LocalPlayerFeed {
        main_hand: item.map(Arc::from),
        first_person: true,
        ..super::cape_tests::feed()
    });
    stream.advance_actor_interpolation_ticks(2);
}

/// Changes an ordinary controller condition through admitted actor metadata.
fn variant(stream: &mut chunk_pipeline::WorldStream, sequence: u64, value: i32) {
    stream
        .submit(
            sequence,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: vec![protocol::ActorMetadata {
                        key: protocol_ids::ActorDataKey::MarkVariant.wire_value(),
                        value: protocol::ActorMetadataValue::Int(value),
                    }]
                    .into(),
                    properties: Arc::from([]),
                    tick: sequence,
                },
            )),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(1);
}

/// Replaces a HUD live-renderer placeholder using the production capture and equipment path.
fn draw(
    ui: &mut UiPresentationRuntime,
    stream: &chunk_pipeline::WorldStream,
    view: PreviewView,
) -> Option<(Arc<ui::UiMesh>, Vec<UiMeshVertex>)> {
    ui.capture_hud_player(Some(stream.authority().local_player_ui()), false);
    draw_captured(ui, view)
}

/// Draws an already captured controller frame through the production UI path.
fn draw_captured(
    ui: &mut UiPresentationRuntime,
    view: PreviewView,
) -> Option<(Arc<ui::UiMesh>, Vec<UiMeshVertex>)> {
    ui.hud_preview_view = view;
    ui.player_preview_view = view;
    let icon = ui.player_preview_icon().unwrap();
    let bounds = crate::ui_runtime::presentation::rect(0., 0., 96., 112.).unwrap();
    let mut nodes =
        vec![
            UiNode::new(UiNodeId::new(1), None, bounds).with_visual(UiVisual::Sprite {
                texture_page: icon.page,
                uv: icon.uv,
                color: [255; 4],
            }),
        ];
    ui.apply_gui_models_in(
        &mut nodes,
        bounds,
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
        0..1,
    );
    match nodes[0].visual() {
        UiVisual::None => None,
        UiVisual::TintedMesh { mesh, .. } => Some((
            Arc::clone(mesh),
            ui.gui_models
                .hud_model
                .lock()
                .unwrap()
                .reference_vertices(mesh),
        )),
        other => panic!("captured live actor left a placeholder: {other:?}"),
    }
}

/// Counts non-collapsed faces, including faces seen edge-on by this preview.
fn triangles(vertices: &[UiMeshVertex]) -> usize {
    vertices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|triangle| {
            let [a, b, c] = triangle.map(|v| Vec3::new(v.position[0], v.position[1], v.clip_z));
            (b - a).cross(c - a).length_squared() > 1e-12
        })
        .count()
}

/// Equips an original cube in the helmet slot without adding armor or held items.
fn wear_helmet_block(ui: &mut UiPresentationRuntime) {
    ui.set_player_preview_gear([None; 4], None);
    let block = PreviewHeldModel {
        alpha_mode: render_model::HandItemAlphaMode::Cutout,
        source: IconRef {
            page: 0,
            uv: [0, 0, 1, 1],
            color_mask: None,
            glint: false,
        },
        vertices: render_model::textured_cube_vertices([[0., 0., 1., 1.]; 6]).into(),
        placements: [PreviewHeldPlacement::Block; 2],
        hand_pivots: [[0.; 3]; 2],
    };
    ui.gui_models.sources.held.insert(
        "minecraft:carved_pumpkin".into(),
        [(0, super::super::held::HeldModel::ready(block))].into(),
    );
    ui.player_preview_gear.head = Some(player_preview::PreviewHandItem {
        shield_pattern: None,
        grip: None,
        glint: Some(false),
        identifier: "minecraft:carved_pumpkin".into(),
        metadata: 0,
        use_animation: Default::default(),
        charged_projectile: Default::default(),
    });
}

/// Draws a helmet block on the captured head and checks it sits on the posed head bone.
fn assert_helmet_block_on_posed_head(view: PreviewView) {
    let pack = pack();
    let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
    equip(&mut stream, None);
    let mut ui = presentation(pack);
    wear_helmet_block(&mut ui);
    let (mesh, vertices) = draw(&mut ui, &stream, view).unwrap();
    let worn = mesh
        .batches()
        .iter()
        .find(|batch| batch.texture_page == 0)
        .expect("the helmet block is drawn");
    let worn = &vertices[worn.index_range.start as usize..worn.index_range.end as usize];
    assert_eq!(triangles(worn), 12);
    let local = stream.authority().local_player_ui();
    let rig = local.rig().unwrap();
    let head = rig
        .bone_names
        .iter()
        .position(|name| name.eq_ignore_ascii_case("head"))
        .unwrap();
    let parent = render_bone(&local.pose().unwrap()[head]).unwrap();
    let posed = render_model::equipment::attach_to_bone(
        parent,
        render_model::equipment::head_block_display(),
    )
    .unwrap();
    let frame = Affine3A::from_scale(
        Vec3::new(-1., 1., -1.) * rig.scale / player_preview::PLAYER_MODEL_SCALE,
    ) * render_matrix(&posed).unwrap();
    let projection = player_preview::Rig::new(Default::default(), view, 0., [false; 2]);
    // A screen's renderer turns the head, and the block with it, toward the pointer; the HUD doll
    // keeps its pose.
    let frame = if view == PreviewView::Hud {
        frame
    } else {
        let (pitch, yaw) = projection.head_look();
        ui.gui_models
            .live_player
            .head_turn(pitch, yaw)
            .map_or(frame, |turn| turn * frame)
    };
    let source = render_model::textured_cube_vertices([[0., 0., 1., 1.]; 6]);
    for (actual, source) in worn.iter().zip(source) {
        let expected = projection.project(render_model::ActorVertex {
            position: frame
                .transform_point3(Vec3::from_array(source.position))
                .to_array(),
            uv: source.uv,
            part: 6,
        });
        for axis in 0..2 {
            let extent = [
                player_preview::PREVIEW_WIDTH,
                player_preview::PREVIEW_HEIGHT,
            ][axis];
            assert!(
                (actual.position[axis] - expected.screen[axis] / extent as f32).abs() < 1e-5,
                "axis {axis}"
            );
        }
    }
}

#[test]
fn live_ui_wears_a_helmet_block_on_the_posed_head_bone() {
    assert_helmet_block_on_posed_head(PreviewView::default());
}

#[test]
fn hud_doll_wears_a_helmet_block_on_the_posed_head_bone() {
    assert_helmet_block_on_posed_head(PreviewView::Hud);
}

#[test]
fn inactive_ui_body_controllers_hide_head_blocks_without_changing_the_hud_doll() {
    let pack = pack();
    let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
    equip(&mut stream, None);
    let mut ui = presentation(pack);
    wear_helmet_block(&mut ui);
    assert!(draw(&mut ui, &stream, PreviewView::default()).is_some());
    variant(&mut stream, 2, 2);
    assert!(draw(&mut ui, &stream, PreviewView::default()).is_none());
    assert!(draw(&mut ui, &stream, PreviewView::Hud).is_some());
    variant(&mut stream, 3, 0);
    assert!(draw(&mut ui, &stream, PreviewView::default()).is_some());
}

#[test]
fn live_ui_hides_body_but_attaches_held_cube_to_animated_item_bone() {
    let pack = pack();
    let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
    equip(&mut stream, Some("minecraft:stone"));
    let local = stream.authority().local_player_ui();
    assert!(local.rig().unwrap().visibility.hides_held_items());
    assert!(!local.layers().unwrap().visibility.hides_held_items());
    assert_ne!(local.rig().unwrap().current, local.pose().unwrap());
    let mut ui = presentation(pack);
    for _ in 0..2 {
        let (mesh, vertices) = draw(&mut ui, &stream, PreviewView::default()).unwrap();
        let item = mesh
            .batches()
            .iter()
            .find(|batch| batch.texture_page == 0)
            .unwrap();
        let item_vertices =
            &vertices[item.index_range.start as usize..item.index_range.end as usize];
        assert_eq!(
            triangles(&vertices),
            12,
            "only the held cube has visible faces"
        );
        assert_eq!(triangles(item_vertices), 12);
        let local = stream.authority().local_player_ui();
        let rig = local.rig().unwrap();
        let hand = rig
            .bone_names
            .iter()
            .position(|name| name.eq_ignore_ascii_case("rightItem"))
            .unwrap();
        let parent = render_bone(&local.pose().unwrap()[hand]).unwrap();
        let mut display = render_model::equipment::held_block_display_for_hand(false);
        display.scale *= local.layers().unwrap().held_item_scale.unwrap();
        let posed = render_model::equipment::attach_to_bone(parent, display).unwrap();
        let frame = Affine3A::from_scale(
            Vec3::new(-1., 1., -1.) * rig.scale / player_preview::PLAYER_MODEL_SCALE,
        ) * render_matrix(&posed).unwrap();
        let projection = player_preview::Rig::new(
            Default::default(),
            PreviewView::default(),
            0.,
            [true, false],
        );
        let source = render_model::textured_cube_vertices([[0., 0., 1., 1.]; 6]);
        for (actual, source) in item_vertices.iter().zip(source) {
            let expected = projection.project(render_model::ActorVertex {
                position: frame
                    .transform_point3(Vec3::from_array(source.position))
                    .to_array(),
                uv: source.uv,
                part: 6,
            });
            assert!(
                (actual.position[0] - expected.screen[0] / player_preview::PREVIEW_WIDTH as f32)
                    .abs()
                    < 1e-5
            );
            assert!(
                (actual.position[1] - expected.screen[1] / player_preview::PREVIEW_HEIGHT as f32)
                    .abs()
                    < 1e-5
            );
        }
        stream.advance_actor_interpolation_ticks(4);
    }
}

#[test]
fn live_ui_visibility_transitions_do_not_restore_a_fallback_body_or_change_the_doll() {
    let pack = pack();
    let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
    equip(&mut stream, Some("minecraft:stone"));
    let mut ui = presentation(pack);
    variant(&mut stream, 2, 2);
    assert!(
        stream
            .authority()
            .local_player_ui()
            .layers()
            .unwrap()
            .visibility
            .hides_body()
    );
    let (_, vertices) = draw(&mut ui, &stream, PreviewView::default()).unwrap();
    assert_eq!(
        triangles(&vertices),
        12,
        "inactive body controllers do not hide held items"
    );
    variant(&mut stream, 3, 3);
    assert!(
        draw(&mut ui, &stream, PreviewView::default()).is_none(),
        "hidden body and item must not reveal the placeholder"
    );
    let (_, doll) = draw(&mut ui, &stream, PreviewView::Hud).unwrap();
    assert!(
        triangles(&doll) > 12,
        "the existing HUD doll retains its full-body contract"
    );
    variant(&mut stream, 4, 1);
    equip(&mut stream, None);
    ui.set_player_preview_gear([None; 4], None);
    let (_, empty_hand) = draw(&mut ui, &stream, PreviewView::default()).unwrap();
    assert_eq!(
        triangles(&empty_hand),
        24,
        "only the arm and sleeve draw for the empty hand"
    );
    equip(&mut stream, Some("minecraft:stone"));
    ui.set_player_preview_gear([None; 4], Some(("minecraft:stone", 0)));
    let (_, held) = draw(&mut ui, &stream, PreviewView::default()).unwrap();
    assert_eq!(
        triangles(&held),
        12,
        "equipping restores only the held cube"
    );
}

#[test]
fn live_ui_draws_alternate_controller_geometry_in_order_with_its_ui_pose_and_mask() {
    let pack = pack_with_alternate(true);
    let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
    equip(&mut stream, Some("minecraft:stone"));
    let output = stream.authority().local_player_ui().layers().unwrap();
    assert!(output.render[0].geometry.is_some());
    let mut ui = presentation(pack);
    ui.set_player_preview_gear([None; 4], None);
    let (_, vertices) = draw(&mut ui, &stream, PreviewView::default()).unwrap();
    assert_eq!(
        triangles(&vertices),
        12,
        "only one unmasked alternate cube draws"
    );
    assert_eq!(
        triangles(&vertices[..72]),
        12,
        "the alternate controller keeps its first draw position"
    );
    assert_eq!(
        triangles(&vertices[72..]),
        0,
        "the ordinary body remains masked"
    );
    let layer = &output.render[0];
    let bone = render_bone(&layer.pose[0]).unwrap();
    assert_ne!(
        bone.rotation,
        [0., 0., 0., 1.],
        "the animation controller also poses alternate geometry in UI"
    );
}

#[test]
fn json_hud_live_player_control_publishes_the_captured_item_palette() {
    let pack = pack();
    let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
    equip(&mut stream, Some("minecraft:stone"));
    let mut ui = presentation(pack);
    let mut files = [
        ("ui/_global_variables.json", "{}"),
        ("ui/_ui_defs.json", r#"{"ui_defs":["ui/hud_screen.json"]}"#),
        (
            "ui/hud_screen.json",
            r#"{"namespace":"hud","hud_screen":{"type":"screen","controls":[
            {"actor":{"type":"custom","renderer":"live_player_renderer","size":[96,112],
                "anchor_from":"bottom_right","anchor_to":"bottom_right"}}
        ]}}"#,
        ),
    ]
    .map(|(path, text)| assets::UiFile {
        path: path.into(),
        bytes: text.as_bytes().into(),
    });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let encoded = assets::encode_ui_catalog(
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
    ui.enable_json_ui_inline(Arc::new(assets::RuntimeUiAssets::decode(&encoded).unwrap()))
        .unwrap();
    ui.hud_frame.player_preview = ui.player_preview_icon();
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    ui.capture_hud_player(Some(stream.authority().local_player_ui()), false);
    let frame = ui
        .build(
            &player,
            &runtime,
            1000,
            [320, 180],
            ui::DpiScale::new(1.).unwrap(),
        )
        .unwrap();
    frame.validate().unwrap();
    assert!(ui.live_renderer_shown());
    assert!(
        !frame.models.is_empty(),
        "the JSON custom control must use the captured actor"
    );
    assert!(
        frame.batches.iter().any(|batch| batch.index_count == 36),
        "the held cube is part of the HUD draw"
    );
}

#[test]
fn live_ui_ordinary_controller_keeps_only_the_primary_texture_sampler() {
    let pack = pack_with_overlay(true, Vec::new(), true);
    let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
    equip(&mut stream, Some("minecraft:stone"));
    let local = stream.authority().local_player_ui();
    let output = local.layers().unwrap();
    assert!(output.render.iter().all(|layer| layer.texture_slot == 0));
    let alternate = output
        .render
        .iter()
        .find(|layer| layer.geometry.is_some())
        .unwrap();
    assert!(alternate.multitexture.is_none());
    assert_eq!(
        pack.assets.sources()[alternate.source as usize]
            .path
            .as_ref(),
        "textures/entity/fixture.png"
    );
    let mut ui = presentation(pack);
    ui.set_player_preview_gear([None; 4], None);
    ui.capture_hud_player(Some(local), false);
    let (_, vertices) = draw_captured(&mut ui, PreviewView::default()).unwrap();
    assert_eq!(
        triangles(&vertices),
        12,
        "secondary samplers do not duplicate controller geometry"
    );
}

#[path = "equipment_tests.rs"]
mod equipment;
