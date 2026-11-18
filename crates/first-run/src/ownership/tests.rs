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

/// A check's answer with the sources and store app its probe offered.
fn checked(answer: Answer, sources: Sources, store_app: Option<&'static str>) -> Checked {
    Checked {
        answer,
        sources,
        store_app,
    }
}

fn dialogs(available: Sources, answers: &[Consent], checks: &[Answer]) -> (bool, Vec<bool>) {
    let script = Script::new(answers);
    let mut remaining: Vec<Answer> = checks.iter().rev().copied().collect();
    let mut sign_ins = Vec::new();
    let owned = with_dialogs(&script, "setup", &mut |sign_in, on_code| {
        sign_ins.push(sign_in);
        if sign_in {
            on_code("https://login.example.test/device", "ABCD-1234");
        }
        checked(remaining.pop().unwrap(), available, None)
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

const PLAY: &str = launcher::ownership::android::STORE;

fn answers(button: &Button) -> bool {
    button.open.is_none() && !button.quit
}

fn hosted(available: Sources, host: &mut Host, checks: &[Answer]) -> (bool, Vec<bool>) {
    let mut remaining: Vec<Answer> = checks.iter().rev().copied().collect();
    let mut sign_ins = Vec::new();
    let owned = with_host(&AtomicBool::new(false), host, &mut |sign_in, on_code| {
        sign_ins.push(sign_in);
        if sign_in {
            on_code("https://login.example.test/device", "ABCD-1234");
        }
        checked(remaining.pop().unwrap(), available, Some(PLAY))
    });
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
    let listing = Some(launcher::ownership::android::STORE_LISTING.to_owned());
    let get = refusal.buttons.iter().find(|button| button.open == listing);
    assert_eq!(get.and_then(|button| button.app.as_deref()), Some(PLAY));
    assert_eq!(
        refusal
            .buttons
            .iter()
            .filter(|button| button.app.is_some())
            .count(),
        1,
        "only Get Minecraft goes to the store app"
    );
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
        &AtomicBool::new(true),
        &mut host,
        &mut |_, _| { checked(Answer::Allowed, PHONE, None) }
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
    let link = Some("https://login.example.test/device".to_owned());
    assert!(code.buttons.iter().any(|button| button.open == link));
}

/// A restored Android game refused by the gate explains ownership, not unfinished setup.
#[test]
fn a_restored_refusal_carries_the_install_guidance_and_notice() {
    let text = restored_refusal(Refusal::NotInstalled, PHONE);
    assert!(text.starts_with(title(Refusal::NotInstalled)));
    assert!(text.contains(install_text(Store::Android)));
    assert!(text.contains("reopen the app"));
    assert!(text.ends_with(NOTICE));
    let label = Screen::Refused {
        refusal: Refusal::NotInstalled,
        sources: PHONE,
    }
    .button_label(Action::Retry);
    assert!(
        !text.contains(label),
        "a restored game has no {label} button"
    );
}

/// An unreadable local install is the unverifiable refusal, whose Try again probes again; any
/// other local failure is not installed.
#[test]
fn a_local_probe_decides_its_refusal() {
    let probe = |install| LocalProbe {
        store_usable: true,
        install,
        store_app: None,
    };
    assert_eq!(
        local_refusal(probe(LocalInstall::Unreadable)),
        Refusal::Unverifiable
    );
    assert_eq!(
        local_refusal(probe(LocalInstall::NotFound)),
        Refusal::NotInstalled
    );
    let actions = refused_actions(Refusal::Unverifiable, PHONE);
    assert_eq!(actions.first(), Some(&Action::Retry));
    let retry = Screen::Refused {
        refusal: Refusal::Unverifiable,
        sources: PHONE,
    }
    .on_action(Action::Retry)
    .1;
    assert_eq!(retry, Effect::Verify { sign_in: false });
}
