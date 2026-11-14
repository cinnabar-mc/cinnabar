use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use launcher::{
    menu::{MenuAction, MenuDialog, auth::AuthState, sign_in::BrowserState},
    pending_sign_in::{self, LeftWaiting},
};

use super::*;

const SAVED_CODE: &str = "SAVED-CODE";

/// Installs a sign-in helper that resumes `SAVED_CODE` and waits for approval.
fn install_resuming_helper(menu: &mut MenuRuntime) {
    let helper = menu.layout.user_data_root.join("resuming-helper.sh");
    fs::create_dir_all(&menu.layout.user_data_root).unwrap();
    fs::write(
        &helper,
        format!(
            "#!/bin/sh\n\
             printf '%s\\n' '{{\"v\":1,\"event\":\"checking_cache\"}}'\n\
             printf '%s\\n' '{{\"v\":1,\"event\":\"device_code\",\"verification_uri\":\"https://example.invalid\",\"user_code\":\"{SAVED_CODE}\"}}'\n\
             exec sleep 30\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    menu.layout.core_executable = helper;
}

/// Installs a sign-in helper that behaves as the real one does around its saved prompt: a
/// token cache that exists is reported as the saved account; otherwise it saves `SAVED_CODE`
/// unless a prompt is already saved, shows the code, and waits for [`approve`]. Approval removes
/// the prompt, writes the token cache and reports the sign-in.
fn install_approving_helper(menu: &mut MenuRuntime) {
    install_helper(menu, false);
}

/// Installs the helper of [`install_approving_helper`]. With `dies_once`, its first run saves
/// and shows the code, then waits for [`end_helper`] and is killed without a terminal event or
/// removing the prompt, as when the system ends the helper process.
fn install_helper(menu: &mut MenuRuntime, dies_once: bool) {
    let helper = menu.layout.user_data_root.join("approving-helper.sh");
    let suffix = pending_sign_in::path(Path::new(""));
    let suffix = suffix.display();
    let dies = if dies_once {
        format!(
            "if [ ! -e \"$cache.died\" ]; then\n\
             mkdir -p \"$(dirname \"$cache\")\"\n\
             : > \"$cache.died\"\n\
             printf '%s' '{{\"device_code\":\"device\",\"user_code\":\"{SAVED_CODE}\",\"verification_uri\":\"https://example.invalid\",\"expires_at\":4102444800}}' > \"$cache{suffix}\"\n\
             mkfifo \"$cache.end\"\n\
             printf '%s\\n' '{{\"v\":1,\"event\":\"checking_cache\"}}'\n\
             printf '%s\\n' '{{\"v\":1,\"event\":\"device_code\",\"verification_uri\":\"https://example.invalid\",\"user_code\":\"{SAVED_CODE}\"}}'\n\
             read ended < \"$cache.end\"\n\
             kill -9 $$\n\
             fi\n"
        )
    } else {
        String::new()
    };
    fs::create_dir_all(&menu.layout.user_data_root).unwrap();
    fs::write(
        &helper,
        format!(
            "#!/bin/sh\n\
             cache=$3\n\
             {dies}\
             printf '%s\\n' '{{\"v\":1,\"event\":\"checking_cache\"}}'\n\
             if [ -e \"$cache\" ]; then\n\
             printf '%s\\n' '{{\"v\":1,\"event\":\"authenticated\",\"method\":\"cached\"}}'\n\
             exit 0\n\
             fi\n\
             mkdir -p \"$(dirname \"$cache\")\"\n\
             [ -e \"$cache{suffix}\" ] || printf '%s' '{{\"device_code\":\"device\",\"user_code\":\"{SAVED_CODE}\",\"verification_uri\":\"https://example.invalid\",\"expires_at\":4102444800}}' > \"$cache{suffix}\"\n\
             rm -f \"$cache.approve\"\n\
             mkfifo \"$cache.approve\"\n\
             printf '%s\\n' '{{\"v\":1,\"event\":\"device_code\",\"verification_uri\":\"https://example.invalid\",\"user_code\":\"{SAVED_CODE}\"}}'\n\
             read approval < \"$cache.approve\"\n\
             rm -f \"$cache{suffix}\"\n\
             printf '{{}}' > \"$cache\"\n\
             printf '%s\\n' '{{\"v\":1,\"event\":\"authenticated\",\"method\":\"device_code\"}}'\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    menu.layout.core_executable = helper;
}

/// Approves the code the helper for `cache` is waiting on, as the player does in the browser.
fn approve(cache: &Path) {
    release(cache, ".approve");
}

/// Ends the first run of a helper installed by [`install_helper`] with `dies_once`.
fn end_helper(cache: &Path) {
    release(cache, ".end");
}

/// The FIFO named by `gate` beside `cache`, on which a test helper waits.
fn gate_path(cache: &Path, gate: &str) -> std::path::PathBuf {
    let mut path = cache.as_os_str().to_owned();
    path.push(gate);
    path.into()
}

/// Releases a test helper waiting on its FIFO named by `gate` beside `cache`.
fn release(cache: &Path, gate: &str) {
    fs::OpenOptions::new()
        .write(true)
        .open(gate_path(cache, gate))
        .unwrap()
        .write_all(b"released\n")
        .unwrap();
}

/// Saves a prompt for `cache` that expires at `expires_at` Unix seconds, as the helper does.
fn save_prompt_until(cache: &Path, expires_at: u64) {
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(
        pending_sign_in::path(cache),
        format!(r#"{{"device_code":"device","user_code":"{SAVED_CODE}","verification_uri":"https://example.invalid","expires_at":{expires_at}}}"#),
    )
    .unwrap();
}

/// Saves a prompt for `cache` that is valid for another hour, as the helper does.
fn save_prompt(cache: &Path) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    save_prompt_until(cache, now.as_secs() + 3600);
}

/// A menu on a system that ends the client while it is hidden, as a phone does.
fn phone_menu() -> MenuRuntime {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.sign_in_away.resumes_at_launch = true;
    menu
}

/// The system ends the client: its helper dies with it and no shutdown code runs, unlike an
/// orderly quit, which drops the runtime and abandons its sign-in. Returns the next launch over
/// the same install.
fn relaunch(mut ended: MenuRuntime) -> MenuRuntime {
    let layout = ended.layout.clone();
    drop(ended.auth_process.take());
    std::mem::forget(ended);
    let mut menu = phone_menu();
    let scratch = std::mem::replace(&mut menu.layout, layout);
    let _ = fs::remove_dir_all(scratch.user_data_root);
    menu
}

/// Polls the menu until the helper's prompt for `SAVED_CODE` is shown.
fn wait_for_saved_prompt(menu: &mut MenuRuntime) {
    test_time::wait_until(Duration::from_secs(5), || {
        menu.poll_catalog(true);
        matches!(menu.current_auth().as_ref(), AuthState::AwaitingCode { code, .. } if code == SAVED_CODE)
    });
}

fn browser_state(menu: &MenuRuntime) -> BrowserState {
    menu.sign_in_browser.state(&AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: SAVED_CODE.into(),
    })
}

fn remove_install(menu: MenuRuntime) {
    let user_data = menu.layout.user_data_root.clone();
    drop(menu);
    fs::remove_dir_all(user_data).unwrap();
}

/// After the phone ended the client mid sign-in, signing in again shows the saved code without
/// sending the player back to the browser they came from. Nothing opens until they ask, and
/// cancelling leaves the code for the next sign-in to resume.
#[test]
fn signing_in_again_resumes_the_saved_code_without_reopening_the_browser() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    install_resuming_helper(&mut menu);
    let cache = menu.layout.auth_cache();
    save_prompt(&cache);
    menu.poll_catalog(true);
    assert!(
        menu.auth_process.is_none(),
        "a launch opened sign-in unasked"
    );
    assert!(!menu.view().sign_in_prompt_open());

    menu.activate(MenuAction::StartSignIn);
    wait_for_saved_prompt(&mut menu);
    assert!(menu.view().sign_in_prompt_open());
    assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
    assert_eq!(browser_state(&menu), BrowserState::Resumed);
    menu.activate(MenuAction::OpenSignInLink);
    assert!(
        !matches!(
            browser_state(&menu),
            BrowserState::Waiting | BrowserState::Resumed
        ),
        "Open link still opens the browser"
    );

    menu.activate(MenuAction::CancelSignIn);
    assert!(pending_sign_in::path(&cache).exists());
    remove_install(menu);
}

/// Add account drops an abandoned attempt's credentials but keeps its unexpired code to resume.
#[test]
fn add_account_resumes_the_saved_code() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    install_resuming_helper(&mut menu);
    let pending = launcher::accounts::AccountStore::new(menu.layout.auth_cache()).pending_cache();
    save_prompt(&pending);

    menu.activate(MenuAction::AddAccount);
    wait_for_saved_prompt(&mut menu);
    assert!(menu.feeds.account_adding);
    assert_eq!(menu.dialog, Some(MenuDialog::Accounts));
    assert!(pending_sign_in::path(&pending).exists());
    assert_eq!(browser_state(&menu), BrowserState::Resumed);
    remove_install(menu);
}

/// Only the resumed code stays out of the browser; a fresh code or an Xbox signup page opens.
#[test]
fn a_resumed_sign_in_still_opens_a_fresh_code_or_a_later_xbox_signup_page() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.sign_in_browser.skip_automatic_open(SAVED_CODE);
    let resumed = AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: SAVED_CODE.into(),
    };
    menu.apply_control_auth(resumed.clone());
    menu.update_sign_in_browser(false);
    assert_eq!(menu.sign_in_browser.state(&resumed), BrowserState::Resumed);
    // The saved code lapsed before the helper read it, so it got a fresh one.
    let fresh = AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "FRESH-CODE".into(),
    };
    menu.apply_control_auth(fresh.clone());
    menu.update_sign_in_browser(false);
    assert_ne!(menu.sign_in_browser.state(&fresh), BrowserState::Waiting);
    let signup = AuthState::AwaitingXboxSignup {
        uri: "https://sisu.xboxlive.com/signup?signature=fixture".into(),
    };
    menu.apply_control_auth(signup.clone());
    menu.update_sign_in_browser(false);
    assert_ne!(menu.sign_in_browser.state(&signup), BrowserState::Waiting);
}

/// The phone ends the client while the player approves the code in the browser. The next launch
/// reopens that sign-in by itself, says it is finishing, keeps the browser closed, and signs
/// the player in once the helper sees the approval, without any tap.
#[test]
fn a_sign_in_the_system_ended_resumes_at_the_next_launch_and_finishes_without_a_tap() {
    let mut menu = phone_menu();
    install_approving_helper(&mut menu);
    let cache = menu.layout.auth_cache();
    menu.activate(MenuAction::StartSignIn);
    wait_for_saved_prompt(&mut menu);
    assert_ne!(
        browser_state(&menu),
        BrowserState::Waiting,
        "a fresh code opens the browser"
    );
    assert_eq!(
        pending_sign_in::left_waiting(&cache, SystemTime::now()),
        Some(LeftWaiting::Awaiting(SAVED_CODE.into())),
        "the code was not noted by the time the browser opened"
    );
    assert_eq!(menu.sign_in_away.hold_requests, [true]);

    let mut menu = relaunch(menu);
    menu.poll_catalog(true);
    assert!(
        menu.auth_process.is_some(),
        "the launch did not resume the sign-in the player was waiting on"
    );
    wait_for_saved_prompt(&mut menu);
    let view = menu.view();
    assert!(view.sign_in_prompt_open());
    assert_eq!(view.sign_in_browser, BrowserState::Resumed);
    assert_eq!(view.focused_action, Some(MenuAction::OpenSignInLink));
    assert_eq!(menu.sign_in_away.hold_requests, [true]);

    approve(&cache);
    test_time::eventually("the approved sign-in to complete", || {
        menu.poll_catalog(true);
        menu.current_auth().as_ref() == &AuthState::Authenticated
    });
    menu.poll_catalog(true);
    assert!(!menu.view().sign_in_prompt_open());
    assert!(!menu.sign_in_requested);
    assert_eq!(
        pending_sign_in::left_waiting(&cache, SystemTime::now()),
        None
    );
    assert_eq!(menu.sign_in_away.hold_requests, [true, false]);
    remove_install(menu);
}

/// A code that lapsed while the client was gone is not reopened; the player starts a new one.
#[test]
fn an_expired_code_is_not_resumed_at_launch() {
    let at = |seconds| UNIX_EPOCH + Duration::from_secs(seconds);
    let mut menu = phone_menu();
    install_resuming_helper(&mut menu);
    let cache = menu.layout.auth_cache();
    save_prompt_until(&cache, 2_000);
    pending_sign_in::note_waiting(&cache, SAVED_CODE).unwrap();
    menu.resume_sign_in_left_waiting(at(2_000));
    assert!(menu.auth_process.is_none(), "an expired code was resumed");
    assert!(!menu.view().sign_in_prompt_open());
    assert_eq!(pending_sign_in::left_waiting(&cache, at(1_999)), None);

    // One second earlier the same code still resumes.
    let mut menu = relaunch(menu);
    pending_sign_in::note_waiting(&cache, SAVED_CODE).unwrap();
    menu.resume_sign_in_left_waiting(at(1_999));
    assert!(menu.auth_process.is_some());
    assert!(menu.sign_in_requested);
    remove_install(menu);
}

/// Only the code this client's player was waiting on resumes: not a prompt saved by another
/// client sharing the install, not one for another attempt, and an Add account's code reopens
/// Add account rather than replacing the signed-in account.
#[test]
fn a_code_saved_for_another_sign_in_is_not_resumed_as_this_one() {
    let mut menu = phone_menu();
    install_resuming_helper(&mut menu);
    let cache = menu.layout.auth_cache();
    save_prompt(&cache);
    menu.poll_catalog(true);
    assert!(
        menu.auth_process.is_none(),
        "a prompt nobody here was waiting on was resumed"
    );

    let mut menu = relaunch(menu);
    pending_sign_in::note_waiting(&cache, "OTHER-CODE").unwrap();
    menu.poll_catalog(true);
    assert!(
        menu.auth_process.is_none(),
        "another attempt's code was resumed"
    );
    assert_eq!(
        pending_sign_in::left_waiting(&cache, SystemTime::now()),
        None
    );
    assert!(pending_sign_in::path(&cache).exists());

    let mut menu = relaunch(menu);
    fs::remove_file(pending_sign_in::path(&cache)).unwrap();
    let adding = menu.account_store().pending_cache();
    save_prompt(&adding);
    pending_sign_in::note_waiting(&adding, SAVED_CODE).unwrap();
    menu.poll_catalog(true);
    wait_for_saved_prompt(&mut menu);
    assert!(menu.feeds.account_adding);
    assert_eq!(menu.dialog, Some(MenuDialog::Accounts));
    assert_eq!(menu.sign_in_cache(), adding);
    assert_eq!(menu.view().sign_in_browser, BrowserState::Resumed);
    remove_install(menu);
}

/// A sign-in the player cancelled stays closed at the next launch, while its code remains for an
/// explicit sign-in to resume.
#[test]
fn a_cancelled_sign_in_stays_closed_at_the_next_launch() {
    let mut menu = phone_menu();
    install_approving_helper(&mut menu);
    let cache = menu.layout.auth_cache();
    menu.activate(MenuAction::StartSignIn);
    wait_for_saved_prompt(&mut menu);
    menu.activate(MenuAction::CancelSignIn);
    assert_eq!(
        pending_sign_in::left_waiting(&cache, SystemTime::now()),
        None
    );
    menu.poll_catalog(true);
    assert!(pending_sign_in::path(&cache).exists());
    assert_eq!(menu.sign_in_away.hold_requests, [true, false]);

    let mut menu = relaunch(menu);
    menu.poll_catalog(true);
    assert!(menu.auth_process.is_none(), "a cancelled sign-in came back");
    assert!(!menu.view().sign_in_prompt_open());
    remove_install(menu);
}

/// The helper gets the token while the client is hidden and draws no frames. When the player
/// returns, the first frames close the prompt and show the account without a tap.
#[test]
fn approval_while_the_client_is_hidden_signs_in_on_return_without_a_tap() {
    let mut menu = phone_menu();
    install_approving_helper(&mut menu);
    let cache = menu.layout.auth_cache();
    menu.activate(MenuAction::StartSignIn);
    wait_for_saved_prompt(&mut menu);
    assert!(menu.view().sign_in_prompt_open());

    // Hidden: the menu is not polled while the helper finishes.
    approve(&cache);
    test_time::eventually("the hidden helper to save the token", || cache.is_file());

    test_time::eventually("the returning client to show the account", || {
        menu.poll_catalog(true);
        menu.current_auth().as_ref() == &AuthState::Authenticated
    });
    menu.poll_catalog(true);
    assert!(!menu.view().sign_in_prompt_open());
    assert_eq!(
        menu.catalog_message.as_deref(),
        Some("Signed in. Loading account destinations…")
    );
    assert_eq!(
        pending_sign_in::left_waiting(&cache, SystemTime::now()),
        None
    );
    assert_eq!(menu.sign_in_away.hold_requests, [true, false]);
    remove_install(menu);
}

/// An Add account's token arrived while the client was hidden, and the system ended the client
/// before the account was added. The next launch finishes adding it instead of discarding the
/// token and asking for a new code.
#[test]
fn an_add_account_approved_while_away_finishes_at_the_next_launch() {
    let mut menu = phone_menu();
    install_approving_helper(&mut menu);
    let adding = menu.account_store().pending_cache();
    fs::create_dir_all(adding.parent().unwrap()).unwrap();
    fs::write(&adding, b"{}").unwrap();
    pending_sign_in::note_waiting(&adding, SAVED_CODE).unwrap();

    menu.poll_catalog(true);
    assert!(menu.feeds.account_adding, "Add account was not resumed");
    assert_eq!(menu.dialog, Some(MenuDialog::Accounts));
    assert!(adding.is_file(), "the arrived token was discarded");
    test_time::eventually("the saved token to be accepted", || {
        menu.poll_catalog(true);
        menu.poll_accounts();
        menu.accounts.pending_ready
    });
    assert_eq!(
        pending_sign_in::left_waiting(&adding, SystemTime::now()),
        Some(LeftWaiting::Settled),
        "the wait was forgotten before the account was added"
    );
    remove_install(menu);
}

/// Where the system keeps a hidden client running, a launch resumes nothing and sign-in writes
/// no note: another client may share the install.
#[test]
fn a_desktop_launch_neither_notes_nor_resumes_a_sign_in() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.sign_in_away.resumes_at_launch = false;
    install_resuming_helper(&mut menu);
    let cache = menu.layout.auth_cache();
    save_prompt(&cache);
    pending_sign_in::note_waiting(&cache, SAVED_CODE).unwrap();
    menu.poll_catalog(true);
    assert!(menu.auth_process.is_none());

    fs::remove_file(pending_sign_in::waiting_path(&cache)).unwrap();
    menu.activate(MenuAction::StartSignIn);
    wait_for_saved_prompt(&mut menu);
    assert!(!pending_sign_in::waiting_path(&cache).exists());
    assert_eq!(
        menu.sign_in_away.hold_requests,
        [true],
        "the platform hold is requested everywhere and only Android acts on it"
    );
    remove_install(menu);
}

/// The system ends only the helper process while the player is in the browser, and the client
/// lives on. The client restarts the helper on the same code without showing its failure or
/// dropping the platform's hold, so approving the code still signs the player in.
#[test]
fn a_helper_the_system_ended_restarts_on_the_same_code() {
    let mut menu = phone_menu();
    install_helper(&mut menu, true);
    let cache = menu.layout.auth_cache();
    menu.activate(MenuAction::StartSignIn);
    wait_for_saved_prompt(&mut menu);
    assert!(pending_sign_in::waiting_path(&cache).exists());

    end_helper(&cache);
    let gate = gate_path(&cache, ".approve");
    test_time::eventually("the restarted helper to wait on the same code", || {
        menu.poll_catalog(true);
        assert!(
            !matches!(menu.current_auth().as_ref(), AuthState::Failed(_)),
            "the stopped helper's failure was shown"
        );
        gate.exists()
            && !menu.auth_restart_requested
            && matches!(menu.current_auth().as_ref(), AuthState::AwaitingCode { code, .. } if code == SAVED_CODE)
    });
    assert!(menu.view().sign_in_prompt_open());

    approve(&cache);
    test_time::eventually("the approved sign-in to complete", || {
        menu.poll_catalog(true);
        menu.current_auth().as_ref() == &AuthState::Authenticated
    });
    menu.poll_catalog(true);
    assert_eq!(menu.sign_in_away.hold_requests, [true, false]);
    remove_install(menu);
}

/// A dialog the launch opened, such as the Minecraft import offer, defers the resume until it
/// closes, so the reopened prompt is never hidden behind it.
#[test]
fn a_launch_dialog_defers_the_resume_until_it_closes() {
    let mut menu = phone_menu();
    install_resuming_helper(&mut menu);
    let cache = menu.layout.auth_cache();
    save_prompt(&cache);
    pending_sign_in::note_waiting(&cache, SAVED_CODE).unwrap();
    menu.dialog = Some(MenuDialog::MinecraftImport);
    menu.poll_catalog(true);
    assert!(
        menu.auth_process.is_none(),
        "a sign-in started behind the dialog"
    );

    menu.dialog = None;
    wait_for_saved_prompt(&mut menu);
    assert!(menu.view().sign_in_prompt_open());
    assert_eq!(menu.view().sign_in_browser, BrowserState::Resumed);
    remove_install(menu);
}
