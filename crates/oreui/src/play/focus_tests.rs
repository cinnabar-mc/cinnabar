use {
    super::*,
    crate::review_tests::paint,
    launcher::menu::{MenuAction, MenuScreen, MenuView, view::LocalWorldCard},
};

#[test]
fn the_first_worlds_focus_outline_fits_inside_the_list_view() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Play;
    view.local_worlds = (0..3)
        .map(|index| LocalWorldCard {
            name: format!("World {index}"),
            game_mode: "survival".into(),
            ..Default::default()
        })
        .collect();
    view.navigation_focus_visible = true;
    view.focused_action = Some(MenuAction::PlayLocalWorld(0));
    let mut ring = 0.0;
    let (scrolls, hits, _) = paint(Default::default(), |canvas| {
        ring = canvas.r(FOCUS_RING);
        draw(canvas, &view, [1280.0, 720.0], &HashMap::new()).unwrap();
    });
    let list = scrolls
        .iter()
        .find(|area| area.key == "worlds_list")
        .expect("the worlds list scrolls")
        .viewport;
    let row = hits
        .iter()
        .find_map(|(action, bounds)| (*action == MenuAction::PlayLocalWorld(0)).then_some(*bounds))
        .expect("the first world is drawn");
    assert!(
        list.min().x() <= row.min().x() - ring
            && list.min().y() <= row.min().y() - ring
            && list.max().x() >= row.max().x() + ring,
        "outline {ring} around {row:?} is clipped by {list:?}"
    );
}
