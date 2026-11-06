//! Incoming history must preserve the reader's position through the full paint path.

use super::*;
use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{Catalog, Context, Draw};
use ui::{DpiScale, SafeArea, TextLayoutCache};
use ui_engine::{
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

struct Offline;
impl RemoteSource for Offline {
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// Anonymous scroll roles and wrapped labels exercise layout without installed game assets.
fn engine() -> FormEngine {
    let bytes = encode_ui_catalog(
        [1; 32],
        &[UiAtlasPage {
            width: 1,
            height: 1,
            rgba8: vec![255; 4].into(),
        }],
        &[],
        &[],
        &[],
    )
    .unwrap();
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/chat_screen.json", r##"{
        "namespace":"chat",
        "chat_screen":{"type":"screen","controls":[{
            "messages_panel":{"type":"scroll_view","size":[220,90],
                "anchor_from":"top_left","anchor_to":"top_left","offset":[20,20],
                "scroll_content":"content","scroll_view_port":"viewport",
                "scrollbar_track":"track","scrollbar_box":"box",
                "scroll_box_and_track_panel":"bars","jump_to_bottom_on_update":true,
                "controls":[
                    {"viewport":{"type":"panel","size":[220,90],
                        "anchor_from":"top_left","anchor_to":"top_left","clips_children":true,
                        "controls":[{"content":{"type":"stack_panel","size":[220,"100%c"],
                            "anchor_from":"top_left","anchor_to":"top_left",
                            "factory":{"name":"messages_factory","control_ids":{"chat_screen_messages":"chat.messages_text"}}}}]}},
                    {"bars":{"type":"panel","size":[8,90],"controls":[
                        {"track":{"type":"scroll_track","size":[8,90]}},
                        {"box":{"type":"scrollbar_box","size":[8,10],"draggable":"vertical"}}
                    ]}}
                ]}
        }]},
        "messages_text":{"type":"label","text":"#text","size":[200,"default"],
            "bindings":[{"binding_name":"#text"}],
            "localize":false,"anchor_from":"top_left","anchor_to":"top_left"}
    }"##);
    let mut registry = ScreenRegistry::new(Context::desktop());
    registry.screen(CHAT_SCREEN);
    FormEngine::new(
        Arc::new(RuntimeUiAssets::decode(&bytes).unwrap()),
        catalog,
        1,
        Box::leak(Box::new(registry)),
        Arc::new(Offline),
        ImageWork::inline(),
    )
}

/// Append a row whose receive FIFO deliberately matches other local notifications.
fn append(store: &mut ui::ChatStore, text: &str) {
    store.push_local(ui::ChatMessage {
        fifo_sequence: 0,
        received_millis: 0,
        kind: ui::ChatMessageKind::System,
        source: None,
        message: text.into(),
        parameters: Arc::from([]),
    });
}

/// Paint at a fixed DPI and return the message positions visible inside the scroll viewport.
fn paint(
    screen: &mut ChatScreen,
    engine: &FormEngine,
    store: &ui::ChatStore,
) -> Vec<(String, f64)> {
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
    let mut nodes = Vec::new();
    let mut next = 1;
    let editor = ui::ChatEditor::new(ui::MAX_CHAT_INPUT_BYTES).unwrap();
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics: TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), Some(2)),
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: [1280.0, 720.0],
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    screen
        .paint(
            engine,
            ChatInput {
                local_player: "Alex",
                editor: &editor,
                messages: store.messages(),
                first_message_id: store.observed_rows().next().map_or(0, |(id, _)| id),
                suggestions: &[],
                usage: None,
                selected_suggestion: None,
                annotations: &[],
                clocks: &BTreeMap::new(),
                now_millis: 0,
            },
            &mut frame,
        )
        .unwrap();
    let (_, metrics) = screen
        .scroll
        .as_ref()
        .expect("history resolves scroll roles");
    assert!(metrics.max_offset() > 0.0);
    screen
        .nodes()
        .iter()
        .filter_map(|node| {
            let Draw::Text { text, .. } = &node.draw else {
                return None;
            };
            let top = metrics.viewport_top;
            (node.dest.y + node.dest.h > top && node.dest.y < top + metrics.viewport)
                .then(|| (text.clone(), node.dest.y))
        })
        .collect()
}

#[test]
fn incoming_wrapped_rows_keep_the_same_messages_in_place() {
    let engine = engine();
    for initial_count in [40, 120, ui::MAX_CHAT_MESSAGES] {
        for pixels in [false, true] {
            let mut store = ui::ChatStore::default();
            for index in 0..initial_count {
                append(&mut store, &format!("Old message {index}"));
            }
            let mut screen = ChatScreen::default();
            paint(&mut screen, &engine, &store);
            screen.scroll(if pixels { 50.0 } else { 4.0 }, pixels);
            let before = paint(&mut screen, &engine, &store);
            assert!(!before.is_empty());
            let (_, old_height) = screen
                .nodes()
                .iter()
                .find_map(|node| {
                    matches!(&node.draw, Draw::Text { .. }).then_some((node.dest.y, node.dest.h))
                })
                .unwrap();
            let old_content = screen.scroll.as_ref().unwrap().1.content;
            for _ in 0..3 {
                append(
                    &mut store,
                    "A newly received message that wraps across multiple lines in the history viewport.",
                );
            }
            let after = paint(&mut screen, &engine, &store);
            let unwrapped_growth = if initial_count < MAX_MESSAGES {
                3.0 * old_height
            } else {
                0.0
            };
            let content = screen.scroll.as_ref().unwrap().1.content;
            assert!(
                content > old_content + unwrapped_growth,
                "the incoming rows must wrap: old={old_content}, new={content}, unwrapped_growth={unwrapped_growth}"
            );
            assert_eq!(
                after, before,
                "initial_count={initial_count}, pixels={pixels}"
            );
            assert_eq!(
                paint(&mut screen, &engine, &store),
                before,
                "the following frame must not drift"
            );
        }
    }
}

#[test]
fn bottom_follow_resumes_after_scrolling_down_and_reopening() {
    let engine = engine();
    let mut store = ui::ChatStore::default();
    for index in 0..40 {
        append(&mut store, &format!("Old message {index}"));
    }
    let mut screen = ChatScreen::default();
    paint(&mut screen, &engine, &store);
    append(&mut store, "Incoming at bottom");
    paint(&mut screen, &engine, &store);
    assert_eq!(screen.from_bottom, 0.0);
    screen.scroll(30.0, true);
    paint(&mut screen, &engine, &store);
    screen.scroll(-10_000.0, true);
    append(&mut store, "Back at bottom");
    paint(&mut screen, &engine, &store);
    assert_eq!(screen.from_bottom, 0.0);
    screen.scroll(30.0, true);
    paint(&mut screen, &engine, &store);
    screen.close();
    paint(&mut screen, &engine, &store);
    assert_eq!(screen.from_bottom, 0.0);
}

#[test]
fn evicted_anchor_clamps_to_the_oldest_retained_message() {
    let engine = engine();
    let mut store = ui::ChatStore::default();
    for index in 0..120 {
        append(&mut store, &format!("Old message {index}"));
    }
    let mut screen = ChatScreen::default();
    paint(&mut screen, &engine, &store);
    screen.scroll(10_000.0, true);
    paint(&mut screen, &engine, &store);
    for index in 0..120 {
        append(&mut store, &format!("New message {index}"));
    }
    let visible = paint(&mut screen, &engine, &store);
    assert!(!visible.is_empty());
    assert_eq!(screen.scroll.as_ref().unwrap().1.offset, 0.0);
}

#[test]
fn incoming_rows_preserve_a_wheel_delta_before_its_next_paint() {
    let engine = engine();
    let mut store = ui::ChatStore::default();
    for index in 0..40 {
        append(&mut store, &format!("Old message {index}"));
    }
    let mut screen = ChatScreen::default();
    paint(&mut screen, &engine, &store);
    screen.scroll(40.0, true);
    let before = paint(&mut screen, &engine, &store);
    let (wanted, y) = before.first().unwrap();
    let delta = screen.scale * 5.0;
    screen.scroll(delta, true);
    append(&mut store, "Incoming before the next frame");
    paint(&mut screen, &engine, &store);
    let after = screen
        .nodes()
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text, .. } if text == wanted))
        .unwrap();
    assert_eq!(after.dest.y, y + 5.0);
}

#[test]
fn a_burst_keeps_surviving_rows_even_when_the_first_layout_culls_them() {
    let engine = engine();
    let mut store = ui::ChatStore::default();
    for index in 0..ui::MAX_CHAT_MESSAGES {
        append(&mut store, &format!("Old message {index}"));
    }
    let mut screen = ChatScreen::default();
    paint(&mut screen, &engine, &store);
    screen.scroll(240.0, true);
    let before = paint(&mut screen, &engine, &store);
    assert!(!before.is_empty());
    for index in 0..20 {
        append(&mut store, &format!("New message {index}"));
    }
    assert_eq!(paint(&mut screen, &engine, &store), before);
}
