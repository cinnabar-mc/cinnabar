use std::cell::RefCell;

use super::*;
use crate::screen::{Effect, Screen};

const LOCAL: Sources = Sources {
    local_install: Some(Store::Microsoft),
    account: false,
};
const ACCOUNT: Sources = Sources {
    local_install: None,
    account: true,
};
const BOTH: Sources = Sources {
    local_install: Some(Store::Microsoft),
    account: true,
};
const PHONE: Sources = Sources {
    local_install: Some(Store::Android),
    account: false,
};

/// Answers each question from a script.
struct Script {
    answers: RefCell<Vec<Consent>>,
}

impl Script {
    fn new(answers: &[Consent]) -> Self {
        Self {
            answers: RefCell::new(answers.iter().rev().copied().collect()),
        }
    }
}

impl Prompter for Script {
    fn confirm(&self, _: &str, _: &str) -> Consent {
        self.answers.borrow_mut().pop().unwrap_or(Consent::Declined)
    }
    fn info(&self, _: &str, _: &str) {}
    fn alert(&self, _: &str, _: &str) {}
}

fn dialogs(available: Sources, answers: &[Consent], checks: &[Answer]) -> (bool, Vec<bool>) {
    let script = Script::new(answers);
    let mut remaining: Vec<Answer> = checks.iter().rev().copied().collect();
    let mut sign_ins = Vec::new();
    let owned = with_dialogs(available, &script, "setup", &mut |sign_in, on_code| {
        sign_ins.push(sign_in);
        if sign_in {
            on_code("https://login.example.test/device", "ABCD-1234");
        }
        remaining.pop().unwrap()
    });
    (owned, sign_ins)
}

#[test]
fn dialogs_open_the_gate_only_when_a_check_passes() {
    assert_eq!(dialogs(LOCAL, &[], &[Answer::Allowed]), (true, vec![false]));
    let missing = Answer::Denied(Refusal::NotInstalled);
    assert_eq!(
        dialogs(LOCAL, &[Consent::Declined], &[missing]),
        (false, vec![false])
    );
    assert_eq!(
        dialogs(LOCAL, &[Consent::Accepted], &[missing, Answer::Allowed]),
        (true, vec![false, false]),
        "Try again checks the install again"
    );
    assert_eq!(
        dialogs(LOCAL, &[Consent::Unavailable], &[missing]),
        (false, vec![false])
    );
}

#[test]
fn dialogs_offer_sign_in_only_with_an_account_source() {
    let required = Answer::Denied(Refusal::SignInRequired);
    assert_eq!(
        dialogs(ACCOUNT, &[Consent::Accepted], &[required, Answer::Allowed]),
        (true, vec![false, true])
    );
    let unverifiable = Answer::Denied(Refusal::Unverifiable);
    assert_eq!(
        dialogs(
            ACCOUNT,
            &[Consent::Accepted],
            &[unverifiable, Answer::Allowed]
        ),
        (true, vec![false, false]),
        "an unverifiable check retries without a new sign-in"
    );
    assert_eq!(
        dialogs(
            ACCOUNT,
            &[Consent::Declined, Consent::Accepted],
            &[unverifiable, Answer::Allowed]
        ),
        (true, vec![false, true]),
        "a stored account that keeps failing can be replaced"
    );
    assert_eq!(
        dialogs(LOCAL, &[Consent::Declined], &[unverifiable]),
        (false, vec![false])
    );
}

#[test]
fn get_minecraft_is_offered_only_when_the_game_is_missing_or_not_owned() {
    for (refusal, available, offered) in [
        (Refusal::NotInstalled, LOCAL, true),
        (Refusal::NotOwned, ACCOUNT, true),
        (Refusal::NotOwned, BOTH, true),
        (Refusal::SignInRequired, ACCOUNT, false),
        (Refusal::SignInRequired, BOTH, false),
        (Refusal::Unverifiable, BOTH, false),
        (Refusal::Unverifiable, LOCAL, false),
    ] {
        let screen = Screen::Refused {
            refusal,
            sources: available,
        };
        assert_eq!(
            screen.actions().contains(&Action::GetMinecraft),
            offered,
            "{refusal:?}"
        );
        assert_eq!(
            screen.actions().contains(&Action::SignIn),
            available.account,
            "{refusal:?}"
        );
        assert!(screen.body().ends_with(NOTICE));
    }
}

#[test]
fn refusal_buttons_check_again_or_sign_in() {
    let missing = Screen::Refused {
        refusal: Refusal::NotInstalled,
        sources: LOCAL,
    };
    assert_eq!(
        missing.on_action(Action::Retry),
        (Some(Screen::Verifying), Effect::Verify { sign_in: false })
    );
    assert_eq!(missing.on_action(Action::SignIn), (None, Effect::None));
    let not_owned = Screen::Refused {
        refusal: Refusal::NotOwned,
        sources: ACCOUNT,
    };
    assert_eq!(
        not_owned.on_action(Action::SignIn),
        (Some(Screen::Verifying), Effect::Verify { sign_in: true })
    );
    assert_eq!(
        not_owned.on_action(Action::GetMinecraft),
        (None, Effect::GetMinecraft)
    );
    let code = Screen::SignIn {
        uri: "https://login.example.test/device".into(),
        code: "ABCD-1234".into(),
    };
    assert_eq!(
        code.on_action(Action::OpenLink),
        (None, Effect::OpenSignInLink)
    );
    assert!(code.body().contains("ABCD-1234"));
}

/// With both sources, declining sign-in must still let the player re-check the local install.
#[test]
fn dialogs_retry_the_local_install_after_declining_sign_in() {
    for refusal in [Refusal::NotOwned, Refusal::SignInRequired] {
        assert_eq!(
            dialogs(
                BOTH,
                &[Consent::Declined, Consent::Accepted],
                &[Answer::Denied(refusal), Answer::Allowed]
            ),
            (true, vec![false, false]),
            "{refusal:?}"
        );
        assert_eq!(
            dialogs(
                BOTH,
                &[Consent::Declined, Consent::Declined],
                &[Answer::Denied(refusal)]
            ),
            (false, vec![false]),
            "{refusal:?}"
        );
    }
}

/// Records what a host was shown; each wait presses the first answering button while presses
/// remain, then closes.
#[derive(Default)]
struct Host {
    progress: usize,
    shown: Vec<Prompt>,
    asked: Vec<Prompt>,
    presses: usize,
}

impl GateHost for Host {
    fn progress(&mut self, _: &Progress) {
        self.progress += 1;
    }
    fn show(&mut self, prompt: &Prompt) {
        self.shown.push(prompt.clone());
    }
    fn choose(&mut self, prompt: &Prompt) -> Option<usize> {
        self.asked.push(prompt.clone());
        self.presses = self.presses.checked_sub(1)?;
        prompt.buttons.iter().position(answers)
    }
}

fn answers(button: &Button) -> bool {
    button.open.is_empty() && !button.quit
}

fn hosted(available: Sources, host: &mut Host, checks: &[Answer]) -> (bool, Vec<bool>) {
    let mut remaining: Vec<Answer> = checks.iter().rev().copied().collect();
    let mut sign_ins = Vec::new();
    let owned = with_host(
        available,
        &AtomicBool::new(false),
        host,
        &mut |sign_in, on_code| {
            sign_ins.push(sign_in);
            if sign_in {
                on_code("https://login.example.test/device", "ABCD-1234");
            }
            remaining.pop().unwrap()
        },
    );
    (owned, sign_ins)
}

#[test]
fn a_host_plays_without_prompts_once_a_check_passes() {
    let mut host = Host::default();
    assert_eq!(
        hosted(PHONE, &mut host, &[Answer::Allowed]),
        (true, vec![false])
    );
    assert_eq!(host.progress, 1);
    assert!(host.asked.is_empty() && host.shown.is_empty());
}

#[test]
fn a_missing_android_install_offers_the_store_listing_then_checks_again() {
    let mut host = Host {
        presses: 1,
        ..Host::default()
    };
    let missing = Answer::Denied(Refusal::NotInstalled);
    assert_eq!(
        hosted(PHONE, &mut host, &[missing, Answer::Allowed]),
        (true, vec![false, false])
    );
    let refusal = &host.asked[0];
    let listing = policy::android::STORE_LISTING.map(str::to_owned);
    assert!(refusal.buttons.iter().any(|button| button.open == listing));
    assert!(refusal.buttons.iter().any(|button| button.quit));
    assert_eq!(
        refusal.buttons.len(),
        refused_actions(Refusal::NotInstalled, PHONE).len()
    );
    assert!(refusal.body.ends_with(NOTICE));
    assert_eq!(host.progress, 2, "each check shows the checking screen");
}

#[test]
fn a_host_that_closes_or_cancels_never_plays() {
    let missing = Answer::Denied(Refusal::NotInstalled);
    let mut closed = Host::default();
    assert_eq!(hosted(PHONE, &mut closed, &[missing]), (false, vec![false]));
    let mut host = Host::default();
    assert!(!with_host(
        PHONE,
        &AtomicBool::new(true),
        &mut host,
        &mut |_, _| Answer::Allowed
    ));
    assert!(host.asked.is_empty());
}

#[test]
fn a_host_signs_in_through_the_shown_code() {
    let mut host = Host {
        presses: 1,
        ..Host::default()
    };
    let required = Answer::Denied(Refusal::SignInRequired);
    assert_eq!(
        hosted(ACCOUNT, &mut host, &[required, Answer::Allowed]),
        (true, vec![false, true])
    );
    let code = &host.shown[0];
    assert!(code.body.contains("ABCD-1234"));
    assert!(
        !code.buttons.iter().any(answers),
        "a running check waits on no answer"
    );
    let link = ["https://login.example.test/device".to_owned()];
    assert!(code.buttons.iter().any(|button| button.open == link));
}
