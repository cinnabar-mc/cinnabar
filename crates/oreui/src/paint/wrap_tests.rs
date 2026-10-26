use super::*;
use ui::oreui_theme::CAPTION;
use ui_engine::test_support::fixture_font;

/// A width rebuilt from a measurement (label plus padding, minus padding) can land a hair under
/// the measured width; the label must still lay out on one line instead of breaking mid-word.
#[test]
fn a_label_given_its_own_measured_width_stays_on_one_line() {
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, TextLayoutCache::new(128, 1024 * 1024));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    for label in ["Failed", "Loading…", "Removing…", "Active"] {
        let width = canvas.measure(label, CAPTION).unwrap();
        let one_line = canvas.measure_height(label, 1000.0, CAPTION).unwrap();
        let rebuilt = width - 0.001;
        assert_eq!(
            canvas.measure_height(label, rebuilt, CAPTION).unwrap(),
            one_line,
            "{label} wrapped at its own measured width",
        );
    }
}
