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

#[test]
fn scrolling_worlds_keeps_play_and_edit_targets_in_the_same_focus_order() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Play;
    view.local_worlds = (0..12)
        .map(|index| LocalWorldCard {
            name: format!("World {index}"),
            ..Default::default()
        })
        .collect();
    let edit = MenuAction::LocalWorld(LocalWorldAction::Edit(0));
    view.navigation_focus_visible = true;
    view.focused_action = Some(edit);
    let expected: Vec<_> = (0..view.local_worlds.len())
        .flat_map(|index| {
            [
                MenuAction::PlayLocalWorld(index),
                MenuAction::LocalWorld(LocalWorldAction::Edit(index)),
            ]
        })
        .collect();
    let mut original_edit = None;
    let (scrolls, hits, _) = paint(Default::default(), |canvas| {
        canvas.capture_focus = true;
        draw(canvas, &view, [1280.0, 720.0], &HashMap::new()).unwrap();
        original_edit = canvas
            .focus_targets
            .iter()
            .find(|target| target.action == edit)
            .copied();
    });
    assert!(hits.iter().any(|(action, _)| *action == edit));
    let original_edit = original_edit.expect("the focused Edit button has a target");
    let area = scrolls
        .iter()
        .find(|area| area.key == "worlds_list")
        .unwrap();
    assert!(area.max > 0.0, "the worlds must overflow the list");
    for offset in [0.0, area.max] {
        let (_, hits, _) = paint(HashMap::from([(area.key.clone(), offset)]), |canvas| {
            canvas.capture_focus = true;
            draw(canvas, &view, [1280.0, 720.0], &HashMap::new()).unwrap();
            let actions: Vec<_> = canvas
                .focus_hits
                .iter()
                .map(|(action, _)| *action)
                .filter(|action| {
                    matches!(
                        action,
                        MenuAction::PlayLocalWorld(_)
                            | MenuAction::LocalWorld(LocalWorldAction::Edit(_))
                    )
                })
                .collect();
            assert_eq!(actions, expected, "world actions at scroll offset {offset}");
            let target = canvas
                .focus_targets
                .iter()
                .find(|target| target.action == edit)
                .expect("scrolling keeps the focused Edit target");
            assert_eq!(target.landmark, original_edit.landmark);
            assert_eq!(target.bounds.min().x(), original_edit.bounds.min().x());
            assert_eq!(target.bounds.max().x(), original_edit.bounds.max().x());
            assert!(
                (target.bounds.min().y() + offset - original_edit.bounds.min().y()).abs() < 0.01
            );
            assert!(
                (target.bounds.max().y() + offset - original_edit.bounds.max().y()).abs() < 0.01
            );
        });
        if offset > 0.0 {
            assert!(
                hits.iter().all(|(action, _)| *action != edit),
                "the row has scrolled out of view"
            );
        }
    }
}
