use super::*;
use launcher::menu::MenuScreen;

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> UiRect {
    UiRect::new(UiPoint::new(x0, y0).unwrap(), UiPoint::new(x1, y1).unwrap()).unwrap()
}

fn landmark(id: u16, bounds: UiRect) -> SettingsFocusLandmark {
    SettingsFocusLandmark {
        id,
        parent: None,
        bounds,
        scroll_axis: None,
        delegate: None,
        delegate_landmark: None,
        remember: true,
        trap: false,
        focus_control_disabled: false,
    }
}

fn target(action: MenuAction, landmark: u16, bounds: UiRect) -> SettingsFocusTarget {
    SettingsFocusTarget {
        action,
        bounds,
        landmark: Some(landmark),
    }
}

const TABS: u16 = 1;
const LIST: u16 = 2;
const DETAILS: u16 = 3;

/// Tabs across the top, a two-row list on the left and a details panel with
/// one Play button on the right; `scroll` is the details panel's (up, down).
fn layout(scroll: (bool, bool)) -> FocusRegions {
    let targets = [
        target(
            MenuAction::Navigate(MenuScreen::Play),
            TABS,
            rect(0.0, 0.0, 100.0, 20.0),
        ),
        target(
            MenuAction::Navigate(MenuScreen::Social),
            TABS,
            rect(100.0, 0.0, 200.0, 20.0),
        ),
        target(
            MenuAction::Navigate(MenuScreen::Servers),
            TABS,
            rect(200.0, 0.0, 300.0, 20.0),
        ),
        target(
            MenuAction::SelectSaved(0),
            LIST,
            rect(0.0, 30.0, 100.0, 50.0),
        ),
        target(
            MenuAction::SelectSaved(1),
            LIST,
            rect(0.0, 50.0, 100.0, 70.0),
        ),
        target(
            MenuAction::PlaySaved(0),
            DETAILS,
            rect(150.0, 40.0, 300.0, 60.0),
        ),
    ];
    let landmarks = [
        landmark(TABS, rect(0.0, 0.0, 300.0, 20.0)),
        landmark(LIST, rect(0.0, 30.0, 100.0, 200.0)),
        landmark(DETAILS, rect(110.0, 30.0, 300.0, 200.0)),
    ];
    let mut regions = FocusRegions::default();
    regions.update(MenuScreen::Servers, &targets, &landmarks, |point| {
        (point.x() > 110.0).then_some(match scroll {
            (false, false) => (0.0, 0.0),
            (false, true) => (0.0, 100.0),
            (true, false) => (100.0, 100.0),
            (true, true) => (50.0, 100.0),
        })
    });
    regions
}

const DOWN: (SettingsFocusAxis, i32) = (SettingsFocusAxis::Vertical, 1);
const UP: (SettingsFocusAxis, i32) = (SettingsFocusAxis::Vertical, -1);
const LEFT: (SettingsFocusAxis, i32) = (SettingsFocusAxis::Horizontal, -1);
const RIGHT: (SettingsFocusAxis, i32) = (SettingsFocusAxis::Horizontal, 1);

fn step(
    regions: &mut FocusRegions,
    from: MenuAction,
    (axis, direction): (SettingsFocusAxis, i32),
) -> Step {
    regions.step(from, axis, direction)
}

#[test]
fn a_column_stops_at_its_last_control() {
    let mut regions = layout((false, false));
    assert_eq!(
        step(&mut regions, MenuAction::SelectSaved(0), DOWN),
        Step::Move(MenuAction::SelectSaved(1))
    );
    assert_eq!(
        step(&mut regions, MenuAction::SelectSaved(1), DOWN),
        Step::Hold
    );
    assert_eq!(regions.take_scroll(), None);
}

#[test]
fn down_past_the_last_control_scrolls_a_region_that_can_scroll() {
    let mut regions = layout((false, true));
    assert_eq!(
        step(&mut regions, MenuAction::PlaySaved(0), DOWN),
        Step::Hold
    );
    let (point, notches) = regions.take_scroll().expect("details scroll down");
    assert!(point.x() > 110.0, "scrolls the details panel");
    assert!(notches < 0.0, "negative wheel notches scroll down");
}

#[test]
fn up_past_the_first_control_scrolls_back_before_leaving_the_region() {
    let mut regions = layout((true, true));
    assert_eq!(step(&mut regions, MenuAction::PlaySaved(0), UP), Step::Hold);
    let (_, notches) = regions.take_scroll().expect("details scroll up");
    assert!(notches > 0.0);
    let mut regions = layout((false, true));
    assert_eq!(
        step(&mut regions, MenuAction::PlaySaved(0), UP),
        Step::Move(MenuAction::Navigate(MenuScreen::Servers)),
        "at the top, up reaches the tab above"
    );
}

#[test]
fn a_row_walks_left_and_right_and_stops_at_its_ends() {
    let mut regions = layout((false, false));
    let social = MenuAction::Navigate(MenuScreen::Social);
    assert_eq!(
        step(&mut regions, social, LEFT),
        Step::Move(MenuAction::Navigate(MenuScreen::Play))
    );
    assert_eq!(
        step(&mut regions, social, RIGHT),
        Step::Move(MenuAction::Navigate(MenuScreen::Servers))
    );
    assert_eq!(
        step(&mut regions, MenuAction::Navigate(MenuScreen::Play), LEFT),
        Step::Hold
    );
    assert_eq!(
        step(&mut regions, MenuAction::Navigate(MenuScreen::Play), UP),
        Step::Hold,
        "nothing above the tabs: focus holds at the top of the screen"
    );
}

#[test]
fn left_and_right_return_to_the_control_last_focused_in_a_region() {
    let mut regions = layout((false, false));
    assert_eq!(
        step(&mut regions, MenuAction::SelectSaved(1), RIGHT),
        Step::Move(MenuAction::PlaySaved(0))
    );
    assert_eq!(
        step(&mut regions, MenuAction::PlaySaved(0), LEFT),
        Step::Move(MenuAction::SelectSaved(1))
    );
    assert_eq!(
        step(&mut regions, MenuAction::SelectSaved(1), LEFT),
        Step::Hold
    );
}

#[test]
fn down_from_a_tab_enters_the_region_beneath_it() {
    let mut regions = layout((false, false));
    assert_eq!(
        step(&mut regions, MenuAction::Navigate(MenuScreen::Play), DOWN),
        Step::Move(MenuAction::SelectSaved(0))
    );
    assert_eq!(
        step(
            &mut regions,
            MenuAction::Navigate(MenuScreen::Servers),
            DOWN
        ),
        Step::Move(MenuAction::PlaySaved(0))
    );
}

#[test]
fn a_row_reads_by_position_when_its_raised_control_is_drawn_last() {
    let targets = [
        target(
            MenuAction::Navigate(MenuScreen::Play),
            TABS,
            rect(0.0, 0.0, 100.0, 20.0),
        ),
        target(
            MenuAction::Navigate(MenuScreen::Servers),
            TABS,
            rect(200.0, 0.0, 300.0, 20.0),
        ),
        target(
            MenuAction::Navigate(MenuScreen::Social),
            TABS,
            rect(100.0, 0.0, 200.0, 20.0),
        ),
    ];
    let mut regions = FocusRegions::default();
    regions.update(
        MenuScreen::Social,
        &targets,
        &[landmark(TABS, rect(0.0, 0.0, 300.0, 20.0))],
        |_| None,
    );
    assert_eq!(
        step(
            &mut regions,
            MenuAction::Navigate(MenuScreen::Social),
            RIGHT
        ),
        Step::Move(MenuAction::Navigate(MenuScreen::Servers))
    );
    assert_eq!(
        step(&mut regions, MenuAction::Navigate(MenuScreen::Social), LEFT),
        Step::Move(MenuAction::Navigate(MenuScreen::Play))
    );
}

#[test]
fn a_lone_control_above_the_tabs_is_reached_by_up_and_left_by_down() {
    const HEADER: u16 = 4;
    let back = MenuAction::Navigate(MenuScreen::Home);
    let targets = [
        target(back, HEADER, rect(0.0, -40.0, 30.0, -10.0)),
        target(
            MenuAction::Navigate(MenuScreen::Play),
            TABS,
            rect(0.0, 0.0, 100.0, 20.0),
        ),
        target(
            MenuAction::Navigate(MenuScreen::Social),
            TABS,
            rect(100.0, 0.0, 200.0, 20.0),
        ),
    ];
    let landmarks = [
        landmark(HEADER, rect(0.0, -40.0, 300.0, -5.0)),
        landmark(TABS, rect(0.0, 0.0, 300.0, 20.0)),
    ];
    let mut regions = FocusRegions::default();
    regions.update(MenuScreen::Social, &targets, &landmarks, |_| None);
    assert_eq!(
        step(&mut regions, MenuAction::Navigate(MenuScreen::Social), UP),
        Step::Move(back)
    );
    assert_eq!(
        step(&mut regions, back, DOWN),
        Step::Move(MenuAction::Navigate(MenuScreen::Social)),
        "down returns to the tab focus left"
    );
}

#[test]
fn another_servers_details_start_again_at_play() {
    let activity = MenuAction::ServerDetail(launcher::menu::view::ServerDetailPart::Activity(1));
    let regions_for = |regions: &mut FocusRegions, server: usize| {
        let targets = [
            target(
                MenuAction::SelectFeatured(0),
                LIST,
                rect(0.0, 30.0, 100.0, 50.0),
            ),
            target(
                MenuAction::SelectFeatured(1),
                LIST,
                rect(0.0, 50.0, 100.0, 70.0),
            ),
            target(
                MenuAction::PlayFeatured(server),
                DETAILS,
                rect(150.0, 40.0, 300.0, 60.0),
            ),
            target(activity, DETAILS, rect(150.0, 70.0, 300.0, 90.0)),
        ];
        let landmarks = [
            landmark(LIST, rect(0.0, 30.0, 100.0, 200.0)),
            landmark(DETAILS, rect(110.0, 30.0, 300.0, 200.0)),
        ];
        regions.update(MenuScreen::Servers, &targets, &landmarks, |_| None);
    };
    let mut regions = FocusRegions::default();
    regions_for(&mut regions, 0);
    assert_eq!(
        step(&mut regions, activity, LEFT),
        Step::Move(MenuAction::SelectFeatured(0))
    );
    assert_eq!(
        step(&mut regions, MenuAction::SelectFeatured(0), RIGHT),
        Step::Move(activity),
        "the same server's details keep their place"
    );
    step(&mut regions, activity, LEFT);
    regions_for(&mut regions, 1);
    assert_eq!(
        step(&mut regions, MenuAction::SelectFeatured(0), RIGHT),
        Step::Move(MenuAction::PlayFeatured(1)),
        "another server's details start at Play"
    );
}

#[test]
fn a_rows_inner_button_is_reached_sideways_and_skipped_up_and_down() {
    let edit = |index| MenuAction::LocalWorld(launcher::menu::LocalWorldAction::Edit(index));
    let mut targets = Vec::new();
    for index in 0..3 {
        let top = index as f32 * 40.0;
        targets.push(target(
            MenuAction::PlayLocalWorld(index),
            LIST,
            rect(0.0, top, 300.0, top + 36.0),
        ));
        targets.push(target(
            edit(index),
            LIST,
            rect(240.0, top + 6.0, 296.0, top + 30.0),
        ));
    }
    let mut regions = FocusRegions::default();
    regions.update(
        MenuScreen::Play,
        &targets,
        &[landmark(LIST, rect(0.0, 0.0, 300.0, 200.0))],
        |_| None,
    );
    let world = MenuAction::PlayLocalWorld;
    assert_eq!(step(&mut regions, world(0), DOWN), Step::Move(world(1)));
    assert_eq!(step(&mut regions, world(1), RIGHT), Step::Move(edit(1)));
    assert_eq!(step(&mut regions, edit(1), RIGHT), Step::Hold);
    assert_eq!(step(&mut regions, edit(1), LEFT), Step::Move(world(1)));
    assert_eq!(step(&mut regions, edit(1), DOWN), Step::Move(world(2)));
    assert_eq!(step(&mut regions, edit(1), UP), Step::Move(world(0)));
    assert_eq!(step(&mut regions, world(2), DOWN), Step::Hold);
}
