//! Annotation geometry remains attached to an eligible name and within the native record budget.

use super::*;
use ui::mod_presentation::{Span, SpanPosition, Spans};

/// Finds one independently colored glyph run.
fn colored_record(scene: &NametagScene, rgb: [f32; 3]) -> &NametagRecord {
    scene
        .records
        .iter()
        .find(|record| {
            record.text != 0
                && record.depth_tested == 0
                && record.color[..3]
                    .iter()
                    .zip(rgb)
                    .all(|(a, b)| (*a - b).abs() < 0.002)
        })
        .unwrap()
}

/// Two spans, the first half transparent, at one position.
fn spans(position: SpanPosition) -> Arc<Spans> {
    Arc::new(Spans {
        spans: [
            Span {
                text: "| ".into(),
                color: [0.2, 0.4, 0.6, 0.5],
            },
            Span {
                text: "-2".into(),
                color: [85. / 255., 1., 85. / 255., 1.],
            },
        ]
        .into(),
        position,
    })
}

/// Actual glyph cells determine inline order, the lines above and below, colour alpha, and the
/// expanded backing plate.
#[test]
fn annotations_preserve_server_lines_tints_and_native_depth_passes() {
    for position in [
        SpanPosition::Before,
        SpanPosition::After,
        SpanPosition::Above,
        SpanPosition::Below,
    ] {
        let mut tag = anchor("Player\n7 wins");
        tag.set_annotation(Some(spans(position)));
        let painted = scene(&[tag]);
        let prefix = colored_record(&painted, [0.2, 0.4, 0.6]);
        assert!((prefix.color[3] - 0.5).abs() < 0.002);
        let count = colored_record(&painted, [85. / 255., 1., 85. / 255.]);
        assert!(prefix.rect[2] <= count.rect[0] + 1.);
        let names: Vec<_> = painted
            .records
            .iter()
            .filter(|record| {
                record.text != 0 && record.depth_tested == 0 && record.color == [1.; 4]
            })
            .collect();
        assert_eq!(
            names.len(),
            2,
            "server name and score remain separate native lines"
        );
        match position {
            SpanPosition::Above => assert!(count.rect[1] < names[0].rect[1]),
            SpanPosition::Below => assert!(count.rect[1] > names[1].rect[1]),
            SpanPosition::Before => assert!(count.rect[0] < names[0].rect[0]),
            SpanPosition::After => assert!(count.rect[0] > names[0].rect[0]),
        }
        assert_eq!(
            painted
                .records
                .iter()
                .filter(|record| record.uv[2] < 0.)
                .count(),
            2 + usize::from(matches!(
                position,
                SpanPosition::Above | SpanPosition::Below
            ))
        );
    }
}

/// Spans on the name's line sit exactly one space advance from it and none from each other;
/// spans on their own line are centred with no space added.
#[test]
fn inline_annotations_keep_one_space_from_the_name() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let mut width = |text: &str| {
        super::super::super::nametag_atlas::line_metrics(text, &font, &mut layouts)
            .unwrap()
            .width_px
    };
    let [name, space, prefix, count] = ["Player", " ", "| ", "-2"].map(&mut width);
    assert!(space > 0.);
    for position in [
        SpanPosition::Before,
        SpanPosition::After,
        SpanPosition::Above,
        SpanPosition::Below,
    ] {
        let mut tag = anchor("Player");
        tag.set_annotation(Some(spans(position)));
        let painted = scene_with_font(&[tag], &font);
        let first = colored_record(&painted, [0.2, 0.4, 0.6]).rect[0];
        let second = colored_record(&painted, [85. / 255., 1., 85. / 255.]).rect[0];
        let named = painted
            .records
            .iter()
            .find(|record| record.text == 1 && record.color == [1.; 4])
            .unwrap()
            .rect[0];
        assert!((second - first - prefix).abs() < 1e-3, "{position:?}");
        let expected = match position {
            SpanPosition::Before => named - (prefix + count + space),
            SpanPosition::After => named + name + space,
            SpanPosition::Above | SpanPosition::Below => {
                render_model::nametag_line_left(prefix + count)
            }
        };
        assert!((first - expected).abs() < 1e-3, "{position:?}");
    }
}

/// A nametag text colour replaces native white text but keeps mod span colours.
#[test]
fn text_colour_keeps_annotation_span_colours() {
    let mut tag = anchor("Player");
    tag.set_annotation(Some(spans(SpanPosition::After)));
    let mut painted = scene(&[tag]);
    let style = ui::mod_presentation::NametagStyle {
        text_color: Some([1., 0., 0., 1.]),
        ..Default::default()
    };
    super::super::style::style_scene(&mut painted, &style, None);
    colored_record(&painted, [0.2, 0.4, 0.6]);
    colored_record(&painted, [1., 0., 0.]);
}

/// Crowded annotated labels use the existing ceiling rather than creating a second unbounded render stream.
#[test]
fn annotation_crowds_respect_nametag_record_limits() {
    let tags: Vec<_> = (0..MAX_PRESENTED_NAMETAGS)
        .map(|n| {
            let mut tag = anchor("Player\n7 wins\nTeam");
            tag.runtime_id = n as u64 + 1;
            tag.set_annotation(Some(spans(SpanPosition::Above)));
            tag
        })
        .collect();
    let painted = scene(&tags);
    assert!(!painted.records.is_empty());
    assert!(painted.records.len() <= MAX_NAMETAG_RECORDS);
}
