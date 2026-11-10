use super::super::super::super::UiRuntime;
use super::*;

fn files() -> Vec<(Box<str>, Vec<u8>)> {
    macro_rules! file {
        ($path:literal) => {
            (
                $path.into(),
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../tools/localserver/testdata/player-preview-layers/",
                    $path
                ))
                .to_vec(),
            )
        };
    }
    vec![
        file!("entity/player.json"),
        file!("models/entity/preview.geo.json"),
        file!("render_controllers/preview.json"),
        file!("animations/preview.json"),
        file!("textures/entity/preview_layers.png"),
    ]
}

fn pack() -> Arc<assets::SessionEntityPack> {
    let compiled = pack_entity_compiler::compile_actor_pack(files())
        .unwrap()
        .unwrap();
    Arc::new(assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment: None,
    })
}

fn presentation(clipped: bool) -> UiPresentationRuntime {
    let mut files = vec![
        assets::UiFile {
            path: "ui/_global_variables.json".into(),
            bytes: b"{}".as_slice().into(),
        },
        assets::UiFile {
            path: "ui/_ui_defs.json".into(),
            bytes: br#"{"ui_defs":["ui/hud_screen.json"]}"#.as_slice().into(),
        },
    ];
    let mut hud: serde_json::Value = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/localserver/testdata/player-preview-layers/ui/hud_screen.json"
    )))
    .unwrap();
    // The fixture's decoration is irrelevant to model coverage and cannot hide a missing layer.
    let player = hud["hud_screen"]["controls"][2].clone();
    hud["hud_screen"]["controls"] = if clipped {
        serde_json::json!([{"clip":{"type":"panel","size":[64,100],"anchor_from":"top_left","anchor_to":"top_left","clip_children":true,"controls":[player]}}])
    } else {
        serde_json::json!([player])
    };
    files.push(assets::UiFile {
        path: "ui/hud_screen.json".into(),
        bytes: serde_json::to_vec(&hud).unwrap().into(),
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
    let mut ui = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
    )
    .unwrap();
    ui.enable_json_ui_inline(Arc::new(assets::RuntimeUiAssets::decode(&encoded).unwrap()))
        .unwrap();
    ui.gui_models.enabled = true;
    ui.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    ui.hud_frame.player_preview = ui.player_preview_icon();
    ui.hud_frame.paper_doll_visible = true;
    ui
}

fn frame(
    ui: &mut UiPresentationRuntime,
    stream: &chunk_pipeline::WorldStream,
) -> render_model::UiRenderInput {
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    ui.capture_hud_player(Some(stream.authority().local_player_ui()), false);
    ui.build(
        &player,
        &UiRuntime::new(1),
        1_000,
        [320, 180],
        ui::DpiScale::new(1.).unwrap(),
    )
    .unwrap()
}

fn colored_pixels(frame: &render_model::UiRenderInput) -> Vec<(usize, usize)> {
    let pixels = super::super::super::forms::snapshot::rasterize(frame);
    pixels
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter_map(|(i, pixel)| {
            (pixel[0] < 70 && pixel[1] > 150 && pixel[2] > 120 && pixel[3] > 0)
                .then_some((i % 320, i / 320))
        })
        .collect()
}

#[test]
fn hud_player_controller_layers_paint_beyond_nominal_control_and_keep_ancestor_clip() {
    let pack = pack();
    let mut stream = super::super::live_player::tests::player_stream(
        Arc::clone(&pack.assets),
        Arc::clone(&pack.assets),
    );
    stream.advance_actor_interpolation_ticks(1);
    let local = stream.authority().local_player_ui();
    assert!(
        local
            .rig()
            .unwrap()
            .render
            .iter()
            .all(|layer| layer.geometry.is_none()),
        "world rendering excludes the HUD controller"
    );
    let output = local.paper_doll_layers().unwrap();
    assert!(
        output.render.iter().any(|layer| layer.geometry.is_some()),
        "HUD context selects the extra geometry"
    );
    let mut ui = presentation(false);
    ui.set_preview_player_layers(Some(Arc::clone(&pack)));
    frame(&mut ui, &stream);
    let rendered = frame(&mut ui, &stream);
    let pixels = colored_pixels(&rendered);
    assert!(
        !pixels.is_empty(),
        "translated controller geometry must paint inside the viewport even when its control is outside"
    );
    assert!(
        pixels.iter().any(|(x, _)| *x > 64),
        "the mesh must retain translation instead of fitting back into its control"
    );
    let mut clipped = presentation(true);
    clipped.set_preview_player_layers(Some(pack));
    frame(&mut clipped, &stream);
    let rendered = frame(&mut clipped, &stream);
    let clipped_pixels = colored_pixels(&rendered);
    assert!(!clipped_pixels.is_empty());
    assert!(
        clipped_pixels.iter().all(|(x, _)| *x < 64),
        "model coverage must obey the ancestor clip"
    );
}

#[test]
fn removing_the_pack_clears_controller_artwork_and_geometry() {
    let pack = pack();
    let mut stream = super::super::live_player::tests::player_stream(
        Arc::clone(&pack.assets),
        Arc::clone(&pack.assets),
    );
    stream.advance_actor_interpolation_ticks(1);
    let mut ui = presentation(false);
    ui.set_preview_player_layers(Some(pack));
    frame(&mut ui, &stream);
    assert!(!colored_pixels(&frame(&mut ui, &stream)).is_empty());
    ui.set_preview_player_layers(None);
    assert!(colored_pixels(&frame(&mut ui, &stream)).is_empty());
    assert!(ui.gui_models.player_layers.pages.is_empty());
    assert!(!ui.gui_models.player_layers.has_layers());
}

#[test]
fn authored_material_culling_reaches_the_retained_controller_palette() {
    for cull in [false, true] {
        let mut source = files();
        let entity = source
            .iter_mut()
            .find(|(path, _)| path.as_ref() == "entity/player.json")
            .unwrap();
        let mut description: serde_json::Value = serde_json::from_slice(&entity.1).unwrap();
        description["minecraft:client_entity"]["description"]["materials"]["default"] =
            serde_json::json!("fixture_preview");
        entity.1 = serde_json::to_vec(&description).unwrap();
        source.push(("materials/preview.material".into(),serde_json::to_vec(&serde_json::json!({"materials":{
            "fixture_preview:entity_alphablend": {"+states": if cull {vec![]} else {vec!["DisableCulling"]}}
        }})).unwrap()));
        let compiled = pack_entity_compiler::compile_actor_pack(source)
            .unwrap()
            .unwrap();
        let pack = Arc::new(assets::SessionEntityPack {
            assets: Arc::new(
                assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap(),
            ),
            textures: compiled.textures.into(),
            bindings: compiled.bindings.into(),
            equipment: None,
        });
        let mut stream = super::super::live_player::tests::player_stream(
            Arc::clone(&pack.assets),
            Arc::clone(&pack.assets),
        );
        stream.advance_actor_interpolation_ticks(1);
        let local = stream.authority().local_player_ui();
        let layers = local.paper_doll_layers().unwrap();
        assert_eq!(layers.render[0].material_state.unwrap().cull, cull);
        let mut ui = presentation(false);
        ui.set_preview_player_layers(Some(pack));
        frame(&mut ui, &stream);
        let rendered = frame(&mut ui, &stream);
        let bones: Vec<_> = rendered
            .models
            .iter()
            .flat_map(|pose| pose.bones.iter())
            .filter(|bone| bone.uv_transform[2] == 2.)
            .collect();
        assert!(!bones.is_empty());
        assert!(bones.iter().all(|bone| bone.backface_culling() == cull));
    }
}

#[test]
fn camera_uv_changes_update_painted_texels_without_changing_geometry() {
    let pack = pack();
    let mut stream = super::super::live_player::tests::player_stream(
        Arc::clone(&pack.assets),
        Arc::clone(&pack.assets),
    );
    stream.advance_actor_interpolation_ticks(1);
    let mut ui = presentation(false);
    ui.set_preview_player_layers(Some(pack));
    frame(&mut ui, &stream);
    let before = frame(&mut ui, &stream);
    let geometry = ui.gui_models.player_layers.layers[0]
        .0
        .geometry
        .as_ref()
        .unwrap()
        .vertices
        .clone();
    stream.set_actor_camera_rotation([0., 45.]);
    stream.advance_actor_interpolation_ticks(1);
    let after = frame(&mut ui, &stream);
    let layer = &ui.gui_models.player_layers.layers[0];
    assert_eq!(layer.1.uv_anim, [0.125, 0., 2., 1.]);
    assert!(Arc::ptr_eq(
        &geometry,
        &layer.0.geometry.as_ref().unwrap().vertices
    ));
    assert_eq!(before.indices, after.indices);
    let raster = super::super::super::forms::snapshot::rasterize;
    assert_ne!(
        raster(&before),
        raster(&after),
        "camera yaw must update the sampled texture phase"
    );
}

#[test]
fn selected_controller_pages_publish_after_an_existing_complete_ui_catalog() {
    let pack = pack();
    let mut stream = super::super::live_player::tests::player_stream(
        Arc::clone(&pack.assets),
        Arc::clone(&pack.assets),
    );
    stream.advance_actor_interpolation_ticks(1);
    let mut ui = presentation(false);
    let mut scene = render_model::UiRenderScene::default();
    let stats = render_model::UiRenderStats::default();
    frame(&mut ui, &stream);
    let before = frame(&mut ui, &stream);
    let original = before.textures.clone();
    scene.publish(before, &stats).unwrap();
    ui.set_preview_player_layers(Some(pack));
    let selected = frame(&mut ui, &stream);
    assert!(selected.textures.pages().len() > original.pages().len());
    assert_ne!(selected.textures.identity(), original.identity());
    scene.publish(selected.clone(), &stats).expect(
        "selected controller art must not invalidate the already admitted fixed UI catalog",
    );
    assert_eq!(
        selected.textures.static_identity(),
        original.static_identity()
    );
    assert!(!colored_pixels(&selected).is_empty());
    ui.set_preview_player_layers(None);
    let removed = frame(&mut ui, &stream);
    scene
        .publish(removed.clone(), &stats)
        .expect("releasing selected controller art must preserve the fixed catalog");
    assert_eq!(removed.textures.pages().len(), original.pages().len());
    assert_eq!(
        removed.textures.static_identity(),
        original.static_identity()
    );
    assert!(colored_pixels(&removed).is_empty());
}

#[test]
fn custom_player_skin_keeps_independent_controller_geometry() {
    let vanilla = pack();
    let mut files = files();
    let mut entity: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let textures = &mut entity["minecraft:client_entity"]["description"]["textures"];
    textures["default"] = serde_json::json!("textures/entity/session_skin");
    textures["panel"] = serde_json::json!("textures/entity/preview_layers");
    files[0].1 = serde_json::to_vec(&entity).unwrap();
    let mut controllers: serde_json::Value = serde_json::from_slice(&files[2].1).unwrap();
    controllers["render_controllers"]["controller.render.preview.panel"]["textures"] =
        serde_json::json!(["Texture.panel"]);
    files[2].1 = serde_json::to_vec(&controllers).unwrap();
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    assert_eq!(compiled.entities.rig_bindings.len(), 1);
    assert_eq!(compiled.textures.len(), 1);
    assert!(
        compiled.bindings.is_empty(),
        "the body image belongs to the session skin"
    );
    let pack = Arc::new(assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment: None,
    });
    let mut stream = super::super::live_player::tests::player_stream(
        Arc::clone(&vanilla.assets),
        Arc::clone(&pack.assets),
    );
    stream.set_pack_entities(Some((Arc::clone(&pack.assets), Vec::new())));
    let skin = protocol::PlayerSkin::Standard(render_api::StandardSkin {
        width:64,height:64,rgba8:vec![255;64*64*4].into(),cape:None,
        geometry:Some(Arc::new(render_api::SkinGeometrySource {
            resource_patch:r#"{"geometry":{"default":"geometry.original_avatar"}}"#.into(),
            geometry_data:r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.original_avatar","texture_width":64,"texture_height":64},"bones":[{"name":"body","pivot":[0,0,0],"cubes":[{"origin":[-3,0,-1],"size":[6,12,2],"uv":[0,0]}]}]}]}"#.into(),
            animations:Arc::from([]),
        })),
    });
    stream
        .submit(
            2,
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid: [0; 16],
                        unique_id: 1,
                        username: "Offline".into(),
                        verified: true,
                        xuid: "".into(),
                        skin,
                    }]),
                },
            )),
        )
        .unwrap();
    test_time::eventually("custom player skin geometry is prepared", || {
        stream.advance_actor_interpolation_frame(0);
        stream
            .authority()
            .local_player_ui()
            .rig()
            .is_some_and(|rig| rig.skin_geometry.is_some())
    });
    stream.advance_actor_interpolation_ticks(1);
    let rig = stream.authority().local_player_ui().rig().unwrap();
    assert!(rig.skin_geometry.is_some());
    assert_eq!(
        rig.geometry_source().unwrap().0.source_manifest_sha256(),
        pack.assets.source_manifest_sha256(),
        "a skin model does not remove the rig's catalog ownership"
    );
    let mut ui = presentation(false);
    ui.set_preview_player_layers(Some(pack));
    frame(&mut ui, &stream);
    let rendered = frame(&mut ui, &stream);
    let mut scene = render_model::UiRenderScene::default();
    scene
        .publish(rendered.clone(), &render_model::UiRenderStats::default())
        .unwrap();
    assert!(
        !colored_pixels(&rendered).is_empty(),
        "the skin-backed avatar must retain the separate controller layer"
    );
}
