use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;
use {
    super::*,
    launcher::menu::{MenuAction, MenuScreen, MenuView},
};

#[test]
fn play_tab_focus_order_stays_left_to_right_when_any_tab_is_raised() {
    let screens = [MenuScreen::Play, MenuScreen::Social, MenuScreen::Servers];
    let expected = screens.map(MenuAction::Navigate);
    for (selected, screen) in screens.into_iter().enumerate() {
        for focused in expected {
            let mut view = MenuView::new(true, "Fixture".into());
            view.screen = screen;
            view.navigation_focus_visible = true;
            view.focused_action = Some(focused);
            crate::review_tests::paint(Default::default(), |canvas| {
                canvas.capture_focus = true;
                draw(canvas, &view, [100.0, 100.0, 1000.0, 160.0], selected).unwrap();
                let actions: Vec<_> = canvas
                    .focus_hits
                    .iter()
                    .map(|(action, _)| *action)
                    .collect();
                assert_eq!(
                    actions, expected,
                    "selected {screen:?}, focused {focused:?}"
                );
                assert_eq!(canvas.hits.len(), expected.len() - 1);
                assert!(
                    canvas
                        .hits
                        .iter()
                        .all(|(action, _)| *action != expected[selected]),
                    "the open tab takes focus without taking clicks"
                );
            });
        }
    }
}

#[test]
fn focused_middle_play_tab_keeps_its_right_outline_above_the_next_tab() {
    use crate::{review_tests::paint, test_support::solids};
    let mut view = MenuView::new(true, "Fixture".into());
    view.navigation_focus_visible = true;
    view.focused_action = Some(MenuAction::Navigate(MenuScreen::Social));
    let (_, _, nodes) = paint(Default::default(), |canvas| {
        draw(canvas, &view, [100.0, 100.0, 1000.0, 160.0], 1).unwrap();
    });
    let (mut nodes_for_metrics, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(1, 1024));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let canvas = Canvas::new(
        &mut nodes_for_metrics,
        &mut next,
        &mut layouts,
        &font,
        metrics,
        0,
        None,
    );
    let edge = canvas.r(EDGE);
    let width = (900.0 + edge * 2.0) / 3.0;
    let x = 100.0 + (width - edge) + width + edge * 0.5;
    let visible = solids(&nodes)
        .into_iter()
        .rev()
        .find(|(b, _)| x >= b[0] && x < b[2] && 130.0 >= b[1] && 130.0 < b[3])
        .unwrap()
        .1;
    assert_eq!(visible, OUTLINE);
}
