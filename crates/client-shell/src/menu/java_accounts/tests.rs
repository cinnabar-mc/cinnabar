use {
    super::*,
    launcher::java_accounts::{JavaAccount, JavaAccountAdd, JavaAccounts},
    launcher::menu::{MenuRealmCard, auth::AuthState, view::MenuFriendCard},
    launcher_host::account_control::AccountEvent,
};

/// An account worker that records the Java account actions it was sent and answers with a
/// queued reply.
#[derive(Default)]
struct FakeAccount {
    sent: Vec<bridge::JavaAccountAction>,
    answer: Option<Result<JavaAccounts, ()>>,
}

impl AccountControl for FakeAccount {
    fn account_status(&mut self) -> Option<AuthState> {
        Some(AuthState::Authenticated)
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
    fn request_java_accounts(&mut self, action: bridge::JavaAccountAction) -> bool {
        self.sent.push(action);
        true
    }
    fn java_accounts(&mut self) -> Option<Result<JavaAccounts, ()>> {
        self.answer.take()
    }
}

/// Two saved Java accounts with the first selected.
fn two_accounts() -> JavaAccounts {
    JavaAccounts {
        accounts: vec![
            JavaAccount {
                id: "a".into(),
                name: "Alex".into(),
            },
            JavaAccount {
                id: "s".into(),
                name: "Steve".into(),
            },
        ],
        selected: Some("a".into()),
        ..Default::default()
    }
}

fn menu() -> MenuRuntime {
    MenuRuntime::new(true, 2, "Fixture".into())
}

/// Opening the Accounts dialog asks the core for the Java accounts, and the answer fills it.
#[test]
fn opening_accounts_lists_the_java_accounts() {
    let mut menu = menu();
    let mut control = FakeAccount::default();
    menu.activate(MenuAction::OpenAccounts);
    let now = Instant::now();
    menu.sync_java_accounts(&mut control, now);
    assert_eq!(control.sent, [bridge::JavaAccountAction::List]);

    control.answer = Some(Ok(two_accounts()));
    menu.sync_java_accounts(&mut control, now);
    assert_eq!(menu.feeds.java_accounts, two_accounts());
}

/// Selecting and removing name the account the dialog shows, by its id.
#[test]
fn java_account_actions_name_the_accounts() {
    let mut menu = menu();
    let mut control = FakeAccount::default();
    menu.activate(MenuAction::OpenAccounts);
    menu.feeds.java_accounts = two_accounts();
    menu.activate(MenuAction::SwitchJavaAccount(1));
    menu.activate(MenuAction::RemoveJavaAccount);
    menu.activate(MenuAction::AddJavaAccount);
    menu.activate(MenuAction::CancelJavaAccount);
    menu.sync_java_accounts(&mut control, Instant::now());
    assert_eq!(
        control.sent,
        [
            bridge::JavaAccountAction::List,
            bridge::JavaAccountAction::Select("s".into()),
            bridge::JavaAccountAction::Remove("a".into()),
            bridge::JavaAccountAction::Add,
            bridge::JavaAccountAction::Cancel,
        ]
    );
}

/// While a Java sign-in waits for its code, the open dialog asks for its progress once a
/// second, never with an earlier request still unanswered, and stops once it is closed.
#[test]
fn an_add_under_way_is_polled_while_the_dialog_shows_it() {
    let mut menu = menu();
    let mut control = FakeAccount::default();
    menu.activate(MenuAction::OpenAccounts);
    let waiting = JavaAccounts {
        add: JavaAccountAdd::AwaitingCode {
            uri: "https://www.microsoft.com/link".into(),
            code: "TESTCODE".into(),
        },
        ..Default::default()
    };
    menu.feeds.java_accounts = waiting.clone();
    let start = Instant::now();
    menu.sync_java_accounts(&mut control, start);
    assert_eq!(control.sent, [bridge::JavaAccountAction::List]);
    menu.sync_java_accounts(&mut control, start + ADD_POLL * 2);
    assert_eq!(
        control.sent.len(),
        1,
        "polled with the last request unanswered"
    );

    control.answer = Some(Ok(waiting));
    menu.sync_java_accounts(&mut control, start + ADD_POLL * 2);
    assert_eq!(control.sent.len(), 2, "no poll once the answer arrived");

    control.answer = Some(Ok(menu.feeds.java_accounts.clone()));
    menu.dialog = None;
    menu.sync_java_accounts(&mut control, start + ADD_POLL * 5);
    assert_eq!(control.sent.len(), 2, "polled with the dialog closed");
}

/// Presentation mode shows placeholder Java accounts: the install's accounts are never asked
/// for, and an answer already on its way does not replace them.
#[test]
fn presentation_mode_shows_placeholder_java_accounts() {
    let mut menu = menu();
    let mut control = FakeAccount::default();
    assert!(menu.set_presentation_accounts(true));
    menu.activate(MenuAction::OpenAccounts);
    control.answer = Some(Ok(two_accounts()));
    menu.sync_java_accounts(&mut control, Instant::now());
    assert!(control.sent.is_empty(), "sent {:?}", control.sent);
    assert_eq!(menu.feeds.java_accounts, presentation_accounts());
}

/// A failed add's reason clears when the dialog closes, and the core forgets it too.
#[test]
fn closing_the_dialog_clears_a_failed_add() {
    let mut menu = menu();
    let mut control = FakeAccount::default();
    menu.activate(MenuAction::OpenAccounts);
    menu.feeds.java_accounts.add = JavaAccountAdd::Failed("The code expired.".into());
    menu.activate(MenuAction::DismissDialog);
    menu.sync_java_accounts(&mut control, Instant::now());
    assert_eq!(menu.feeds.java_accounts.add, JavaAccountAdd::Idle);
    assert_eq!(
        control.sent,
        [
            bridge::JavaAccountAction::List,
            bridge::JavaAccountAction::Cancel
        ]
    );
}

/// A core that cannot be asked leaves the last list and says so.
#[test]
fn an_unreachable_core_keeps_the_list_and_reports_it() {
    let mut menu = menu();
    let mut control = FakeAccount::default();
    menu.feeds.java_accounts = two_accounts();
    control.answer = Some(Err(()));
    menu.sync_java_accounts(&mut control, Instant::now());
    assert_eq!(menu.feeds.java_accounts.accounts.len(), 2);
    assert_eq!(menu.feeds.java_accounts.error.as_deref(), Some(UNREACHABLE));
}
