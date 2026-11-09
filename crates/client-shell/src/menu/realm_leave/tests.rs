use {
    super::*,
    launcher::menu::{MenuRealmCard, MenuScreen, REALM_ADDRESS_PREFIX, view::MenuFriendCard},
    launcher_host::account_control::{AccountEvent, RealmLeaveResponse},
};

/// Builds a signed-in launcher on the Realms tab listing an owned and a joined Realm.
fn menu() -> MenuRuntime {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.control_auth = Some(AuthState::Authenticated);
    menu.enter(MenuScreen::Social);
    menu.realms = vec![realm("Mine", 3, false), realm("Joined", 7, true)].into();
    menu
}

/// A listed Realm with a stable service id.
fn realm(name: &str, id: u32, member: bool) -> MenuRealmCard {
    MenuRealmCard {
        name: name.into(),
        target: format!("{REALM_ADDRESS_PREFIX}{id}"),
        state: "OPEN".into(),
        address: String::new(),
        owner: String::new(),
        online_players: 0,
        max_players: 10,
        days_left: 0,
        expired: false,
        member,
    }
}

/// An account worker that records sent leaves and answers each with a queued outcome.
#[derive(Default)]
struct LeaveAccount {
    generation: u64,
    sent: Vec<(u64, u32)>,
    outcome: Option<RealmLeaveResponse>,
}

impl AccountControl for LeaveAccount {
    fn account_status(&mut self) -> Option<AuthState> {
        Some(AuthState::Authenticated)
    }
    fn account_generation(&mut self) -> Option<u64> {
        Some(self.generation)
    }
    fn realms(&mut self) -> Option<Vec<MenuRealmCard>> {
        None
    }
    fn friends(&mut self) -> Option<Vec<MenuFriendCard>> {
        None
    }
    fn sign_out(&mut self) -> bool {
        false
    }
    fn poll_event(&mut self) -> Option<AccountEvent> {
        None
    }
    fn request_realm_leave(&mut self, ticket: u64, realm_id: u32) -> bool {
        self.sent.push((ticket, realm_id));
        true
    }
    fn realm_leave(&mut self) -> Option<RealmLeaveResponse> {
        self.outcome.take()
    }
}

/// The names of the Realms the menu still lists.
fn names(menu: &MenuRuntime) -> Vec<&str> {
    menu.realms
        .iter()
        .map(|realm| realm.name.as_str())
        .collect()
}

/// Draws the normal OreUI route without installed game assets.
fn draw(menu: &MenuRuntime) -> (Vec<MenuAction>, Vec<String>) {
    let mut presentation = client_ui::ui_runtime::presentation::UiPresentationRuntime::new(
        ui_engine::test_support::fixture_font(),
    )
    .unwrap();
    presentation.set_menu_view(Some(menu.view()));
    let mut nodes = Vec::new();
    let mut next = 1;
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let hits = presentation
        .append_menu(
            &client_ui::ui_runtime::UiRuntime::new(1),
            &mut nodes,
            &mut next,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    let texts = client_ui::test_support::pack_harness::drawn_texts(&nodes);
    (hits.into_iter().map(|(action, _)| action).collect(), texts)
}

/// Selects the Realm list and details controls from either focus or painted hits.
fn realm_controls(actions: impl IntoIterator<Item = MenuAction>) -> Vec<MenuAction> {
    actions
        .into_iter()
        .filter(|action| {
            matches!(
                action,
                MenuAction::SelectRealm(_)
                    | MenuAction::PlayRealm(_)
                    | MenuAction::LeaveRealmDialog(_)
            )
        })
        .collect()
}

#[test]
fn realms_focus_matches_enabled_oreui_controls() {
    for owned_first in [false, true] {
        for selected in [None, Some(0), Some(1), Some(usize::MAX)] {
            for pending in [false, true] {
                let mut menu = menu();
                if !owned_first {
                    menu.realms.reverse();
                }
                menu.feeds.selected_realm = selected;
                if pending {
                    menu.realm_leave.pending = Some((1, 7, "Joined".into()));
                }
                let (drawn, _) = draw(&menu);
                assert_eq!(
                    realm_controls(menu.focus_actions()),
                    realm_controls(drawn),
                    "owned_first={owned_first}, selected={selected:?}, pending={pending}"
                );
            }
        }
    }

    let mut menu = menu();
    menu.realms = vec![realm("Joined", 7, true)].into();
    for state in ["OPEN", "CLOSED"] {
        for target in [format!("{REALM_ADDRESS_PREFIX}7"), String::new()] {
            let realm = &mut menu.realms[0];
            realm.state = state.into();
            realm.target = target;
            let (drawn, _) = draw(&menu);
            assert_eq!(realm_controls(menu.focus_actions()), realm_controls(drawn));
        }
    }
}

#[test]
fn navigation_selects_a_joined_realm_and_opens_its_leave_confirmation() {
    let mut menu = menu();
    for _ in 0..menu.focus_actions().len() {
        if menu.focused_action() == Some(MenuAction::SelectRealm(1)) {
            break;
        }
        menu.move_focus(1);
    }
    assert_eq!(menu.focused_action(), Some(MenuAction::SelectRealm(1)));
    menu.activate_focused();
    for _ in 0..menu.focus_actions().len() {
        if menu.focused_action() == Some(MenuAction::LeaveRealmDialog(7)) {
            break;
        }
        menu.move_focus(1);
    }
    assert_eq!(menu.focused_action(), Some(MenuAction::LeaveRealmDialog(7)));
    assert!(draw(&menu).0.contains(&MenuAction::LeaveRealmDialog(7)));
    menu.activate_focused();
    assert_eq!(menu.dialog, Some(MenuDialog::LeaveRealm(7)));
}

#[test]
fn leaving_needs_a_confirmed_dialog_and_never_offers_an_owned_realm() {
    let mut menu = menu();
    let mut account = LeaveAccount::default();
    menu.activate(MenuAction::ConfirmLeaveRealm(7));
    menu.activate(MenuAction::LeaveRealmDialog(3));
    assert_eq!(menu.dialog, None, "an owned Realm cannot be left");
    menu.sync_realm_leave(&mut account);
    assert!(account.sent.is_empty(), "leaving must be confirmed first");

    menu.activate(MenuAction::LeaveRealmDialog(7));
    assert_eq!(menu.dialog, Some(MenuDialog::LeaveRealm(7)));
    menu.activate(MenuAction::DismissDialog);
    menu.sync_realm_leave(&mut account);
    assert!(account.sent.is_empty(), "cancelling must not leave");

    menu.activate(MenuAction::LeaveRealmDialog(7));
    menu.activate(MenuAction::ConfirmLeaveRealm(7));
    assert_eq!(menu.dialog, None);
    menu.sync_realm_leave(&mut account);
    assert_eq!(account.sent.len(), 1);
    assert_eq!(account.sent[0].1, 7);
    assert_eq!(menu.view().leaving_realm, Some(7));
}

#[test]
fn a_successful_leave_removes_only_that_realm() {
    let mut menu = menu();
    let mut account = LeaveAccount::default();
    menu.feeds.selected_realm = Some(1);
    menu.activate(MenuAction::LeaveRealmDialog(7));
    menu.activate(MenuAction::ConfirmLeaveRealm(7));
    menu.sync_realm_leave(&mut account);
    menu.activate(MenuAction::LeaveRealmDialog(7));
    assert_eq!(
        menu.dialog, None,
        "a second leave waits for the first outcome"
    );
    account.outcome = Some((account.sent[0].0, Ok(())));
    menu.sync_realm_leave(&mut account);
    assert_eq!(names(&menu), ["Mine"]);
    assert_eq!(menu.feeds.selected_realm, None);
    assert_eq!(menu.view().leaving_realm, None);
}

#[test]
fn a_failed_leave_shows_an_oreui_notice_and_allows_a_retry() {
    let mut menu = menu();
    let mut account = LeaveAccount::default();
    menu.activate(MenuAction::LeaveRealmDialog(7));
    menu.activate(MenuAction::ConfirmLeaveRealm(7));
    menu.sync_realm_leave(&mut account);
    account.outcome = Some((account.sent[0].0, Err(())));
    menu.sync_realm_leave(&mut account);
    assert_eq!(names(&menu), ["Mine", "Joined"]);
    assert_eq!(menu.dialog, Some(MenuDialog::Notice));
    let (actions, texts) = draw(&menu);
    assert!(
        texts
            .iter()
            .any(|text| text == "Unable to leave Joined. Please try again.")
    );
    assert_eq!(actions, [MenuAction::DismissDialog]);
    assert_eq!(menu.focus_actions(), actions);
    menu.activate_focused();
    assert_eq!(menu.dialog, None);
    menu.activate(MenuAction::LeaveRealmDialog(7));
    assert_eq!(menu.dialog, Some(MenuDialog::LeaveRealm(7)));
}

#[test]
fn an_account_change_discards_the_pending_leave_outcome() {
    let mut menu = menu();
    let mut account = LeaveAccount::default();
    menu.activate(MenuAction::LeaveRealmDialog(7));
    menu.activate(MenuAction::ConfirmLeaveRealm(7));
    menu.sync_realm_leave(&mut account);
    let ticket = account.sent[0].0;
    account.generation += 1;
    menu.sync_realm_leave(&mut account);
    assert_eq!(menu.view().leaving_realm, None);
    account.outcome = Some((ticket, Ok(())));
    menu.sync_realm_leave(&mut account);
    assert_eq!(names(&menu), ["Mine", "Joined"]);
}
