//! Retained container layouts across language and scale changes.

use super::*;
#[test]
fn review_container_layout_remeasures_after_language_or_scale_changes() {
    struct Text(f64);
    impl json_ui::TextMeasure for Text {
        fn extent(&self, _: &str) -> [f64; 2] {
            [self.0, 10.0]
        }
    }
    struct NoTexture;
    impl json_ui::TextureSource for NoTexture {
        /// The measurement fixture has no image sources.
        fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
            None
        }
    }
    let mut catalog = json_ui::Catalog::default();
    catalog.apply_pack([("ui/_ui_defs.json", br#"{"ui_defs":["ui/chest.json"]}"#.as_slice()), ("ui/chest.json", br#"{"namespace":"chest","small_chest_screen":{"type":"screen","controls":[{"label":{"type":"label","text":"key","size":["default",10]}}]}}"#.as_slice())]);
    let catalog = Arc::new(catalog);
    let mut cache = None;
    let (context, data, view) = (
        json_ui::Context::desktop(),
        DataSource::new(),
        ViewState::default(),
    );
    for (generation, width) in [(0, 10.0), (1, 30.0)] {
        let text = Text(width);
        let env = json_ui::LayoutEnv {
            text: &text,
            textures: &NoTexture,
        };
        let rendered = ScreenCache::render(
            &mut cache,
            &catalog,
            &catalog,
            "chest.small_chest_screen",
            &context,
            &data,
            &view,
            [100.0; 2],
            &env,
            (1.0, [generation, 0, 0]),
        )
        .unwrap();
        let label = rendered
            .nodes
            .iter()
            .find(|node| node.name == "label")
            .unwrap();
        assert_eq!(label.dest.w, width);
    }
}
