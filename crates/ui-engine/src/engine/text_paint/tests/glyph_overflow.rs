use super::*;

/// Original bitmap art with an icon extending above and below an ordinary line.
fn icon_font() -> RuntimeFontCatalog {
    let pixels: Arc<[u8]> = vec![255; 16 * 32 * 4].into();
    let page = FontTexturePage {
        source_path: "font/overflow-fixture.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [5; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 16,
        height: 32,
        pixels: FontPixels::Rgba8(pixels),
    };
    let glyphs = ['0', '\u{e100}', '\u{fffd}'].map(|codepoint| {
        let icon = codepoint == '\u{e100}';
        GlyphMetrics {
            codepoint,
            page: 0,
            uv: [0, 0, 12, if icon { 32 } else { 16 }],
            bearing: [0, if icon { -22 } else { -14 }],
            advance_64: 12 * 64,
        }
    });
    let bytes = encode_font_catalog([5; 32], &glyphs, &[page]).unwrap();
    RuntimeFontCatalog::decode(&bytes, [5; 32]).unwrap()
}

#[test]
fn oversized_bitmap_icons_keep_label_boxes_and_text_baselines() {
    let font = icon_font();
    for dpi in [1.0, 1.5] {
        let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(dpi).unwrap(), Some(2));
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        for factor in [0.5, 1.0, 2.0] {
            for (plain, decorated) in [("00", "\u{e100}0"), ("00\n00", "\u{e100}0\n00")] {
                let json = serde_json::json!({"namespace":"fixture", "screen":{
                    "type":"stack_panel", "orientation":"horizontal", "controls":
                    ([plain, decorated].map(|text| serde_json::json!({"entry":{
                        "type":"button", "size":["100%cm", "100%cm"], "controls":[
                            {"caption":{"type":"label", "text":text, "localize":false,
                                "font_scale_factor":factor, "line_padding":2,
                                "anchor_from":"top_left", "anchor_to":"top_left"}}
                        ]
                    }})))
                }})
                .to_string();
                let (frame, nodes) = paint_fixture(&font, metrics, &json);
                assert_eq!(frame.hits.len(), 2);
                let [plain_box, icon_box] = [frame.hits[0].rect, frame.hits[1].rect];
                assert_eq!(plain_box.h, icon_box.h, "DPI {dpi}, factor {factor}");
                assert_eq!(
                    plain_box.w, icon_box.w,
                    "icon advance still contributes width"
                );
                let layouts: Vec<_> = nodes
                    .iter()
                    .filter_map(|node| match node.visual() {
                        UiVisual::Text { layout, .. } => Some(layout),
                        _ => None,
                    })
                    .collect();
                assert_eq!(layouts.len(), 2);
                assert_eq!(layouts[0].size_64(), layouts[1].size_64());
                assert_eq!(
                    layouts[0].glyphs()[1].bounds_64,
                    layouts[1].glyphs()[1].bounds_64,
                    "ordinary text must keep its baseline beside an icon"
                );
                let icon = layouts[1].glyphs()[0];
                assert!(icon.bounds_64[1] < 0, "icon may paint above its line box");
                assert!(
                    icon.bounds_64[3] as f32 / 64.0 > TEXT_PITCH as f32 * factor as f32 * px,
                    "icon may paint below its line box"
                );
                assert!(!layouts[1].ellipsized());
                let mut tree = UiTree::new(nodes).unwrap();
                tree.layout(
                    rect(0., 0., 1280., 720.).unwrap(),
                    UiScale::default(),
                    SafeArea::ZERO,
                )
                .unwrap();
                let draw = tree.build_draw_list().unwrap();
                let tallest = draw
                    .vertices
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|quad| {
                        let top = quad
                            .iter()
                            .map(|vertex| vertex.position[1])
                            .fold(f32::INFINITY, f32::min);
                        let bottom = quad
                            .iter()
                            .map(|vertex| vertex.position[1])
                            .fold(f32::NEG_INFINITY, f32::max);
                        bottom - top
                    })
                    .fold(0., f32::max);
                assert!(
                    (tallest - 32. * metrics.scale.get() * factor as f32).abs() < 0.1,
                    "the full icon must reach the draw list"
                );
            }
        }
    }
}
