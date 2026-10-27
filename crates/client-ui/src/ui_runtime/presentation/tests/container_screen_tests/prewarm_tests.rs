use super::*;

use crate::ui_runtime::presentation::forms::ServerUiPack;
use serde_json::json;

const FIXED_LABELS: usize = 512;

/// Produces a large static branch whose first bind must run during preparation.
fn fixed_labels() -> Vec<serde_json::Value> {
    (0..FIXED_LABELS)
        .map(|index| {
            json!({format!("fixed_{index}"): {
                "type":"label", "size":[12,12], "text":"200"
            }})
        })
        .collect()
}

/// Installs an original inventory definition over the in-memory engine carrier.
fn inventory_fixture(
    definition: serde_json::Value,
) -> (UiPresentationRuntime, UiRuntime, player_state::PlayerState) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = session(&mut player);
    runtime.set_server_ui(Some(Arc::new(ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/crafting.json"]}"#.to_vec(),
            ),
            (
                "ui/crafting.json".into(),
                serde_json::to_vec(&definition).unwrap(),
            ),
        ]],
        ..Default::default()
    })));
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.gui_scale_preference = Some(1);
    (presentation, runtime, player)
}

/// Builds at the caller's animation time with a fixed GUI scale and viewport.
fn draw(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    player: &player_state::PlayerState,
    now: u64,
) -> UiRenderInput {
    let mut frame = draw_frame(presentation, runtime, player, now);
    if !runtime.inventory_open() {
        test_time::eventually("inventory preparation worker", || {
            frame = draw_frame(presentation, runtime, player, now);
            !presentation.engine_inventory_preparation_pending()
        });
    }
    frame
}

/// Builds exactly one frame, allowing tests to observe unfinished preparation.
fn draw_frame(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    player: &player_state::PlayerState,
    now: u64,
) -> UiRenderInput {
    presentation
        .build(
            player,
            runtime,
            now,
            [320, 240],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

/// Publishes one occupied hotbar cell; names remain caller-controlled frame data.
fn populate(runtime: &mut UiRuntime, player: &mut player_state::PlayerState) {
    runtime
        .enqueue_inventory_event(
            player,
            runtime.session_id(),
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: vec![NetworkItemStack {
                    network_id: 1,
                    count: 1,
                    stack_network_id: 1,
                    ..NetworkItemStack::empty()
                }]
                .into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(player);
}

/// A server label that captures its hotbar cell's hover name only once.
fn once_name_screen() -> serde_json::Value {
    json!({"namespace":"crafting", "inventory_screen": {
        "type":"screen", "controls":[{"slot":{
            "type":"panel", "size":[100,12], "collection_name":"hotbar_items",
            "controls":[{"name":{
                "type":"label", "size":[100,12], "text":"#text", "collection_index":0,
                "bindings":[{
                    "binding_type":"collection", "binding_collection_name":"hotbar_items",
                    "binding_name":"#hover_text", "binding_name_override":"#text",
                    "binding_condition":"once"
                }]
            }}]
        }}]
    }})
}

#[test]
fn prewarmed_once_binding_reads_the_name_at_first_open() {
    let (mut warm, mut runtime, mut player) = inventory_fixture(once_name_screen());
    let (mut cold, _, _) = inventory_fixture(once_name_screen());
    populate(&mut runtime, &mut player);
    let closed = draw(&mut warm, &runtime, &player, 0);
    assert_eq!(warm.engine_container_resolves(), 1);
    assert!(warm.engine_container_frame().is_none());
    assert!(warm.hud_frame().item_names.is_empty());

    runtime.toggle_inventory(&mut player, None);
    for presentation in [&mut warm, &mut cold] {
        presentation
            .hud_frame_mut()
            .item_names
            .insert((1, 0), Arc::from("200"));
    }
    let expected = draw(&mut cold, &runtime, &player, 100);
    let opened = draw(&mut warm, &runtime, &player, 100);
    assert!(cold.engine_container_frame().is_some());
    assert!(warm.engine_container_frame().is_some());
    assert!(
        !expected.vertices.is_empty(),
        "the once-bound name must draw"
    );
    assert_ne!(opened.vertices, closed.vertices);
    assert_eq!(
        opened.vertices, expected.vertices,
        "prewarm must not capture an empty name"
    );
    assert_eq!(
        warm.engine_container_resolves(),
        1,
        "opening reuses the resolved screen"
    );

    warm.hud_frame_mut()
        .item_names
        .insert((1, 0), Arc::from("00"));
    let refreshed = draw(&mut warm, &runtime, &player, 200);
    assert_eq!(
        refreshed.vertices, opened.vertices,
        "the real open must retain its once binding"
    );
}

#[test]
fn first_inventory_open_reuses_prepared_static_bindings() {
    let mut definition = once_name_screen();
    let controls = definition["inventory_screen"]["controls"]
        .as_array_mut()
        .unwrap();
    controls.extend(fixed_labels());
    let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
    let (mut cold, _, _) = inventory_fixture(definition);
    populate(&mut runtime, &mut player);
    draw(&mut warm, &runtime, &player, 0);
    let prepared = warm.engine_container_placed();
    let prepared_bindings = warm.engine_inventory_rebuilt();
    assert!(prepared >= FIXED_LABELS);

    runtime.toggle_inventory(&mut player, None);
    for presentation in [&mut warm, &mut cold] {
        presentation
            .hud_frame_mut()
            .item_names
            .insert((1, 0), Arc::from("00"));
    }
    let expected = draw(&mut cold, &runtime, &player, 100);
    let opened = draw(&mut warm, &runtime, &player, 100);
    assert_eq!(opened.vertices, expected.vertices);
    let rebuilt = warm.engine_inventory_rebuilt() - prepared_bindings;
    assert!(
        rebuilt < 16,
        "first open rebuilt {rebuilt} prepared controls"
    );
    let placed = warm.engine_container_placed() - prepared;
    assert!(placed < 32, "first open placed {placed} prepared controls");
}

#[test]
fn late_inventory_inputs_prepare_newly_visible_controls_before_first_open() {
    let definition = json!({"namespace":"crafting", "inventory_screen":{
        "type":"screen", "controls":[{"creative":{
            "type":"panel", "size":["100%","100%"],
            "bindings":[{"binding_name":"#is_creative_mode",
                "binding_name_override":"#visible"}],
            "controls":fixed_labels()
        }}]
    }});
    let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
    let (mut cold, _, _) = inventory_fixture(definition);
    draw(&mut warm, &runtime, &player, 0);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    draw(&mut warm, &runtime, &player, 50);
    let prepared_bindings = warm.engine_inventory_rebuilt();
    assert!(warm.engine_container_frame().is_none());
    runtime.toggle_inventory(&mut player, None);
    let expected = draw(&mut cold, &runtime, &player, 100);
    let opened = draw(&mut warm, &runtime, &player, 100);
    assert_eq!(opened.vertices, expected.vertices);
    let rebuilt = warm.engine_inventory_rebuilt() - prepared_bindings;
    assert!(rebuilt < 16, "first open rebuilt {rebuilt} late controls");
}

#[test]
fn prepared_scroll_feedback_keeps_unchanged_binding_lifetimes() {
    let mut labels = fixed_labels();
    for label in &mut labels {
        let label = label.as_object_mut().unwrap().values_mut().next().unwrap();
        label["bindings"] = json!([{
            "binding_name":"#is_creative_mode", "binding_name_override":"#remembered",
            "binding_condition":"once"
        }]);
    }
    labels.push(json!({"scroll":{
        "type":"scroll_view", "size":[100,100],
        "scroll_content":"content", "scroll_view_port":"viewport",
        "scrollbar_track":"track", "scrollbar_box":"box",
        "scroll_box_and_track_panel":"bars",
        "controls":[{"viewport":{
            "type":"panel", "size":[100,100], "controls":[{"content":{
                "type":"panel", "size":[100,300]
            }}]
        }},{"bars":{
            "type":"panel", "size":[8,100], "controls":[
                {"track":{"type":"panel", "size":[8,100]}},
                {"box":{"type":"scrollbar_box", "size":[8,10], "draggable":"vertical"}}
            ]
        }}]
    }}));
    labels.push(json!({"reader":{
        "type":"label", "size":[12,12], "text":"200",
        "bindings":[{"binding_type":"view", "source_control_name":"scroll",
            "source_property_name":"#scrolled_to_end", "target_property_name":"#at_end"}]
    }}));
    let definition = json!({"namespace":"crafting", "inventory_screen":{
        "type":"screen", "controls":labels
    }});
    let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
    let (mut cold, _, _) = inventory_fixture(definition);
    draw(&mut warm, &runtime, &player, 0);
    let prepared = warm.engine_inventory_rebuilt();
    runtime.toggle_inventory(&mut player, None);
    let expected = draw(&mut cold, &runtime, &player, 100);
    let opened = draw(&mut warm, &runtime, &player, 100);
    assert_eq!(opened.vertices, expected.vertices);
    let expected_feedback = draw(&mut cold, &runtime, &player, 150);
    let feedback = draw(&mut warm, &runtime, &player, 150);
    assert_eq!(feedback.vertices, expected_feedback.vertices);
    let rebuilt = warm.engine_inventory_rebuilt() - prepared;
    assert!(
        rebuilt < 16,
        "scroll feedback rebuilt {rebuilt} prepared controls"
    );
}

#[test]
fn late_inventory_contents_prepare_newly_visible_controls_before_first_open() {
    let definition = json!({"namespace":"crafting", "inventory_screen":{
        "type":"screen", "controls":[{"slot":{
            "type":"panel", "size":[100,100], "collection_name":"hotbar_items",
            "controls":[{"occupied":{
                "type":"panel", "size":[100,100], "collection_index":0,
                "bindings":[
                    {"binding_type":"collection", "binding_collection_name":"hotbar_items",
                        "binding_name":"#hover_text", "binding_name_override":"#caption"},
                    {"binding_type":"view", "source_property_name":"(#caption = '200')",
                        "target_property_name":"#visible"}
                ],
                "controls":fixed_labels()
            }}]
        }}]
    }});
    let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
    let (mut cold, _, _) = inventory_fixture(definition);
    draw(&mut warm, &runtime, &player, 0);
    populate(&mut runtime, &mut player);
    for presentation in [&mut warm, &mut cold] {
        presentation
            .hud_frame_mut()
            .item_names
            .insert((1, 0), Arc::from("200"));
    }
    draw(&mut warm, &runtime, &player, 50);
    let prepared = warm.engine_inventory_rebuilt();
    runtime.toggle_inventory(&mut player, None);
    let expected = draw(&mut cold, &runtime, &player, 100);
    let opened = draw(&mut warm, &runtime, &player, 100);
    assert_eq!(opened.vertices, expected.vertices);
    let rebuilt = warm.engine_inventory_rebuilt() - prepared;
    assert!(
        rebuilt < 16,
        "late contents rebuilt {rebuilt} prepared controls"
    );
}

#[test]
fn closing_an_inventory_opened_without_prewarm_keeps_its_once_binding() {
    let (mut presentation, mut runtime, mut player) = inventory_fixture(once_name_screen());
    populate(&mut runtime, &mut player);
    runtime.toggle_inventory(&mut player, None);
    presentation
        .hud_frame_mut()
        .item_names
        .insert((1, 0), Arc::from("200"));
    let first = draw(&mut presentation, &runtime, &player, 0);
    assert!(!first.vertices.is_empty());
    runtime.toggle_inventory(&mut player, None);
    presentation.hud_frame_mut().item_names.clear();
    draw(&mut presentation, &runtime, &player, 100);
    runtime.toggle_inventory(&mut player, None);
    presentation
        .hud_frame_mut()
        .item_names
        .insert((1, 0), Arc::from("00"));
    let reopened = draw(&mut presentation, &runtime, &player, 200);
    assert_eq!(
        reopened.vertices, first.vertices,
        "closing must not reset live once bindings"
    );
}

#[test]
fn prewarmed_inventory_animation_starts_at_the_real_open() {
    for wait_until_rendered in [false, true] {
        let definition = json!({"namespace":"crafting",
            "opening":{"anim_type":"alpha", "duration":1.0, "from":0.25, "to":1.0,
                "wait_until_rendered_to_play":wait_until_rendered},
            "inventory_screen":{"type":"screen", "controls":[{"animated":{
                "type":"label", "size":[100,12], "text":"200", "alpha":"@crafting.opening"
            }}]}
        });
        let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
        let (mut cold, _, _) = inventory_fixture(definition);
        draw(&mut warm, &runtime, &player, 0);
        assert_eq!(warm.engine_container_resolves(), 1);
        assert!(warm.engine_container_frame().is_none());
        runtime.toggle_inventory(&mut player, None);
        let mut first = None;
        for now in [100, 300, 600] {
            let expected = draw(&mut cold, &runtime, &player, now);
            let opened = draw(&mut warm, &runtime, &player, now);
            assert!(!expected.vertices.is_empty());
            assert_eq!(
                opened.vertices, expected.vertices,
                "animation phase at {now}; wait_until_rendered={wait_until_rendered}"
            );
            if let Some(first) = &first {
                assert_ne!(
                    &opened.vertices, first,
                    "the opening animation must advance"
                );
            } else {
                first = Some(opened.vertices);
            }
        }
        assert_eq!(warm.engine_container_resolves(), 1);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn closed_inventory_frames_defer_binding_and_layout() {
    let definition = json!({"namespace":"crafting", "inventory_screen":{
        "type":"screen", "controls":fixed_labels()
    }});
    let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
    let (mut cold, _, _) = inventory_fixture(definition);
    draw_frame(&mut warm, &runtime, &player, 0);
    assert_eq!(warm.engine_container_resolves(), 0);
    assert_eq!(warm.engine_container_placed(), 0);
    assert_eq!(warm.engine_inventory_rebuilt(), 0);
    assert!(warm.engine_inventory_preparation_pending());
    // Opening immediately must finish the worker's remaining work and paint the same screen.
    runtime.toggle_inventory(&mut player, None);
    let expected = draw_frame(&mut cold, &runtime, &player, 100);
    let opened = draw_frame(&mut warm, &runtime, &player, 100);
    assert_eq!(opened.vertices, expected.vertices);
    assert_eq!(warm.engine_container_resolves(), 1);
    assert!(!warm.engine_inventory_preparation_pending());
}

#[test]
fn unchanged_closed_inventory_does_no_more_preparation() {
    let definition = json!({"namespace":"crafting", "inventory_screen":{
        "type":"screen", "controls":fixed_labels()
    }});
    let (mut warm, runtime, player) = inventory_fixture(definition);
    draw(&mut warm, &runtime, &player, 0);
    let work = (
        warm.engine_container_resolves(),
        warm.engine_container_placed(),
        warm.engine_inventory_rebuilt(),
    );
    for now in 1..8 {
        draw_frame(&mut warm, &runtime, &player, now);
        assert!(!warm.engine_inventory_preparation_pending());
        assert_eq!(
            (
                warm.engine_container_resolves(),
                warm.engine_container_placed(),
                warm.engine_inventory_rebuilt()
            ),
            work
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn image_residency_keeps_prepared_inventory_measurements() {
    let keys = ["textures/ui/residency_0", "textures/ui/residency_1"];
    let mut controls = fixed_labels();
    for (index, key) in keys.iter().enumerate() {
        controls.push(json!({format!("image_{index}"):{
            "type":"image", "texture":key, "size":[16,16]
        }}));
    }
    let definition = json!({"namespace":"crafting", "inventory_screen":{
        "type":"screen", "controls":controls
    }});
    let (mut presentation, runtime, player) = inventory_fixture(definition);
    draw(&mut presentation, &runtime, &player, 0);

    let width = ui_engine::pages::WHOLE_ART_PAGE + 1;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, 2, image::Rgba([20, 40, 60, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let files: Vec<_> = keys
        .iter()
        .map(|key| (format!("{key}.png"), bytes.get_ref().clone()))
        .collect();
    let engine = presentation.engine.as_mut().unwrap();
    engine.textures = ui_engine::textures::TextureSet::new(
        0,
        Arc::clone(&engine.textures.remote),
        ui_engine::sources::ImageWork::default(),
    );
    engine
        .textures
        .set_atlas(ui_engine::atlas::ServerAtlas::new(&files, None, 1), 1);
    let head = render_model::UiTexturePage::owned([4, 2], vec![255; 32].into()).unwrap();
    engine
        .textures
        .atlas_mut()
        .set_local_player_head(Some(&head));
    presentation.poll_server_ui_images();
    draw(&mut presentation, &runtime, &player, 1);
    let placed = presentation.engine_container_placed();
    let metadata = presentation
        .engine
        .as_ref()
        .unwrap()
        .textures
        .metadata_revision();
    test_time::eventually("all prepared inventory images become resident", || {
        draw_frame(&mut presentation, &runtime, &player, 2);
        let atlas = presentation.engine.as_ref().unwrap().textures.lock();
        keys.iter().all(|key| atlas.native_sheet_ready(key))
            && !presentation.engine_inventory_preparation_pending()
    });
    assert_eq!(
        presentation
            .engine
            .as_ref()
            .unwrap()
            .textures
            .metadata_revision(),
        metadata
    );
    assert_eq!(
        presentation.engine_container_placed(),
        placed,
        "image readiness must request later images without placing controls again"
    );
    assert!(!presentation.engine_inventory_preparation_pending());

    let head = render_model::UiTexturePage::owned([8, 2], vec![255; 64].into()).unwrap();
    presentation
        .engine
        .as_mut()
        .unwrap()
        .textures
        .atlas_mut()
        .set_local_player_head(Some(&head));
    presentation.poll_server_ui_images();
    assert!(
        presentation.inventory_preparation_needed(&player, &runtime),
        "metadata published before the poll still invalidates preparation"
    );
}

#[test]
fn opening_during_a_source_change_uses_the_current_inputs() {
    let definition = json!({"namespace":"crafting", "inventory_screen":{
        "type":"screen", "controls":[{"creative":{
            "type":"panel", "size":["100%","100%"],
            "bindings":[{"binding_name":"#is_creative_mode",
                "binding_name_override":"#visible"}],
            "controls":fixed_labels()
        }}]
    }});
    let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
    let (mut cold, _, _) = inventory_fixture(definition);
    draw_frame(&mut warm, &runtime, &player, 0);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    draw_frame(&mut warm, &runtime, &player, 1);
    runtime.toggle_inventory(&mut player, None);
    let expected = draw_frame(&mut cold, &runtime, &player, 2);
    let opened = draw_frame(&mut warm, &runtime, &player, 2);
    assert_eq!(opened.vertices, expected.vertices);
    assert!(warm.engine_container_frame().is_some());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn replacing_the_catalog_drops_obsolete_preparation_before_open() {
    let old = json!({"namespace":"crafting", "inventory_screen":{
        "type":"screen", "controls":fixed_labels()
    }});
    let replacement = once_name_screen();
    let (mut warm, mut runtime, mut player) = inventory_fixture(old);
    let (mut cold, replacement_runtime, _) = inventory_fixture(replacement);
    draw_frame(&mut warm, &runtime, &player, 0);
    assert!(warm.engine_inventory_preparation_pending());
    runtime.set_server_ui(replacement_runtime.server_ui().cloned());
    runtime.toggle_inventory(&mut player, None);
    let expected = draw_frame(&mut cold, &runtime, &player, 100);
    let opened = draw_frame(&mut warm, &runtime, &player, 100);
    assert_eq!(opened.vertices, expected.vertices);
    assert_eq!(
        warm.engine_container_resolves(),
        1,
        "opening must resolve only the replacement, without finishing the obsolete catalog"
    );
    assert!(!warm.engine_inventory_preparation_pending());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_source_changes_reuse_static_measurements_at_first_open() {
    let mut definition = once_name_screen();
    definition["inventory_screen"]["controls"]
        .as_array_mut()
        .unwrap()
        .extend(fixed_labels());
    let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
    let (mut prepared, _, _) = inventory_fixture(definition);
    draw(&mut prepared, &runtime, &player, 0);
    let initial_placed = prepared.engine_container_placed();
    assert!(initial_placed >= FIXED_LABELS);
    draw_frame(&mut warm, &runtime, &player, 0);
    assert!(warm.engine_inventory_preparation_pending());
    populate(&mut runtime, &mut player);
    draw_frame(&mut warm, &runtime, &player, 1);
    runtime.toggle_inventory(&mut player, None);
    let expected = draw_frame(&mut prepared, &runtime, &player, 2);
    let opened = draw_frame(&mut warm, &runtime, &player, 2);
    assert_eq!(opened.vertices, expected.vertices);
    assert!(
        warm.engine_container_placed() < initial_placed + 32,
        "a ledger refresh discarded prepared static measurements: {} -> {}",
        initial_placed,
        warm.engine_container_placed()
    );
}

#[test]
fn rejoining_prepares_a_fresh_inventory_lifetime_before_open() {
    let mut definition = once_name_screen();
    definition["inventory_screen"]["controls"]
        .as_array_mut()
        .unwrap()
        .extend(fixed_labels());
    let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
    let (mut cold, _, _) = inventory_fixture(definition);
    populate(&mut runtime, &mut player);
    warm.hud_frame_mut()
        .item_names
        .insert((1, 0), Arc::from("200"));
    draw(&mut warm, &runtime, &player, 0);
    runtime.toggle_inventory(&mut player, None);
    draw(&mut warm, &runtime, &player, 1);
    runtime.toggle_inventory(&mut player, None);

    let pack = runtime.server_ui().cloned();
    runtime.begin_session(2);
    player = player_state::PlayerState::new(runtime.session_id());
    runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(&mut player, runtime.session_id(), 42)
        .unwrap();
    runtime.set_server_ui(pack);
    populate(&mut runtime, &mut player);
    for presentation in [&mut warm, &mut cold] {
        presentation
            .hud_frame_mut()
            .item_names
            .insert((1, 0), Arc::from("00"));
        draw(presentation, &runtime, &player, 2);
    }
    let placed = warm.engine_container_placed();
    let bound = warm.engine_inventory_rebuilt();
    runtime.toggle_inventory(&mut player, None);
    let expected = draw(&mut cold, &runtime, &player, 3);
    let opened = draw(&mut warm, &runtime, &player, 3);
    assert_eq!(
        opened.vertices, expected.vertices,
        "a new session reads fresh once bindings"
    );
    assert!(warm.engine_container_placed() - placed < 32);
    assert!(warm.engine_inventory_rebuilt() - bound < 16);
}
