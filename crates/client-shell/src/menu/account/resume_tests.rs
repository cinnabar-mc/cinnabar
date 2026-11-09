use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};

use launcher::menu::{MenuAction, MenuDialog, auth::AuthState, sign_in::BrowserState};

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

/// Saves a prompt for `cache` that is valid for another hour, as the helper does.
fn save_prompt(cache: &Path) {
    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(
        launcher::pending_sign_in::path(cache),
        format!(r#"{{"device_code":"device","user_code":"{SAVED_CODE}","verification_uri":"https://example.invalid","expires_at":{expires_at}}}"#),
    )
    .unwrap();
}

/// Polls the menu until the helper's resumed prompt is shown.
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
    assert_eq!(browser_state(&menu), BrowserState::Waiting);
    menu.activate(MenuAction::OpenSignInLink);
    assert_ne!(
        browser_state(&menu),
        BrowserState::Waiting,
        "Open link still opens the browser"
    );

    menu.activate(MenuAction::CancelSignIn);
    assert!(launcher::pending_sign_in::path(&cache).exists());
    let user_data = menu.layout.user_data_root.clone();
    drop(menu);
    fs::remove_dir_all(user_data).unwrap();
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
    assert!(launcher::pending_sign_in::path(&pending).exists());
    assert_eq!(browser_state(&menu), BrowserState::Waiting);
    let user_data = menu.layout.user_data_root.clone();
    drop(menu);
    fs::remove_dir_all(user_data).unwrap();
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
    assert_eq!(menu.sign_in_browser.state(&resumed), BrowserState::Waiting);
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
