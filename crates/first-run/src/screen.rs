//! The setup window's state: what each first-run report shows and what each button does.

use std::time::{Duration, Instant};

use launcher::ownership::Sources;

use super::{
    CONSENT_BODY,
    ownership::{self, Refusal},
    status::{Phase, Status},
};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Screen {
    /// The ownership gate is checking its sources.
    Verifying,
    /// A license account sign-in waits on the player's browser.
    SignIn {
        uri: String,
        code: String,
    },
    /// The ownership gate stayed closed.
    Refused {
        refusal: Refusal,
        sources: Sources,
    },
    Consent,
    /// Accepted or retried; no report has arrived yet.
    Starting,
    Downloading {
        received: u64,
        total: Option<u64>,
        bytes_per_second: Option<f64>,
    },
    Preparing {
        step: usize,
        total: usize,
        label: String,
    },
    Failed {
        message: String,
    },
    Done,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Accept,
    ViewEula,
    Retry,
    SignIn,
    OpenLink,
    GetMinecraft,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Effect {
    None,
    StartWorker,
    OpenEula,
    /// Runs the ownership gate again, through a new license sign-in when `sign_in`.
    Verify {
        sign_in: bool,
    },
    OpenSignInLink,
    GetMinecraft,
    Quit,
}

impl Screen {
    /// Buttons in display order; the first non-Quit one is the Enter default.
    pub(super) const fn actions(&self) -> &'static [Action] {
        match self {
            Self::Consent => &[Action::Accept, Action::ViewEula, Action::Quit],
            Self::Verifying => &[Action::Quit],
            Self::SignIn { .. } => &[Action::OpenLink, Action::Quit],
            Self::Refused { refusal, sources } => refused_actions(*refusal, *sources),
            Self::Failed { .. } => &[Action::Retry, Action::Quit],
            Self::Done => &[],
            Self::Starting | Self::Downloading { .. } | Self::Preparing { .. } => &[Action::Quit],
        }
    }

    pub(super) fn primary(&self) -> Option<Action> {
        self.actions()
            .first()
            .copied()
            .filter(|action| *action != Action::Quit)
    }

    /// The next screen (if it changes) and the side effect of pressing `action`.
    pub(super) fn on_action(&self, action: Action) -> (Option<Self>, Effect) {
        if !self.actions().contains(&action) {
            return (None, Effect::None);
        }
        match action {
            Action::Retry if matches!(self, Self::Refused { .. }) => {
                (Some(Self::Verifying), Effect::Verify { sign_in: false })
            }
            Action::Accept | Action::Retry => (Some(Self::Starting), Effect::StartWorker),
            Action::SignIn => (Some(Self::Verifying), Effect::Verify { sign_in: true }),
            Action::OpenLink => (None, Effect::OpenSignInLink),
            Action::GetMinecraft => (None, Effect::GetMinecraft),
            Action::ViewEula => (None, Effect::OpenEula),
            Action::Quit => (None, Effect::Quit),
        }
    }

    pub(super) fn button_label(&self, action: Action) -> &'static str {
        match (action, self) {
            (Action::Accept, _) => "Accept and continue",
            (Action::ViewEula, _) => "View EULA",
            (Action::Retry, Self::Refused { .. }) => "Try again",
            (Action::Retry, _) => "Retry",
            (
                Action::SignIn,
                Self::Refused {
                    refusal: Refusal::NotOwned | Refusal::Unverifiable,
                    ..
                },
            ) => "Use another account",
            (Action::SignIn, _) => "Sign in",
            (Action::OpenLink, _) => "Open link",
            (Action::GetMinecraft, _) => "Get Minecraft",
            (Action::Quit, Self::Starting | Self::Downloading { .. } | Self::Preparing { .. }) => {
                "Cancel"
            }
            (Action::Quit, _) => "Quit",
        }
    }

    /// The heading; `updating` swaps first-time wording for update wording.
    pub(super) const fn title(&self, updating: bool) -> &'static str {
        match (self, updating) {
            (Self::Verifying, _) => "Checking that you own Minecraft…",
            (Self::SignIn { .. }, _) => "Sign in with Microsoft",
            (Self::Refused { refusal, .. }, _) => ownership::title(*refusal),
            (Self::Consent, false) => "First-time setup",
            (Self::Consent, true) => "Cinnabar needs updated resources",
            (Self::Starting | Self::Downloading { .. } | Self::Preparing { .. }, true) => {
                "Updating Minecraft resources…"
            }
            (Self::Starting, false) => "Starting setup…",
            (Self::Downloading { .. }, false) => "Downloading Minecraft resources…",
            (Self::Preparing { .. }, false) => "Preparing assets…",
            (Self::Failed { .. }, false) => "Setup failed",
            (Self::Failed { .. }, true) => "Update failed",
            (Self::Done, _) => "Starting Cinnabar…",
        }
    }

    /// Body text; paragraphs split on `\n`.
    pub(super) fn body(&self) -> String {
        match self {
            Self::Consent => CONSENT_BODY.to_owned(),
            Self::SignIn { uri, code } => ownership::code_prompt(uri, code),
            Self::Refused { refusal, sources } => ownership::body(*refusal, *sources),
            Self::Downloading {
                received,
                total,
                bytes_per_second,
            } => download_detail(*received, *total, *bytes_per_second),
            Self::Preparing { step, total, label } => format!("Step {step} of {total}: {label}"),
            Self::Failed { message } => message.clone(),
            Self::Verifying | Self::Starting | Self::Done => String::new(),
        }
    }

    /// Fraction done, when the screen has a progress bar.
    pub(super) fn progress(&self) -> Option<f32> {
        match self {
            Self::Starting => Some(0.0),
            Self::Downloading {
                received, total, ..
            } => Some(total.filter(|total| *total > 0).map_or(0.0, |total| {
                (*received as f64 / total as f64).min(1.0) as f32
            })),
            Self::Preparing { step, total, .. } if *total > 0 => {
                Some(step.saturating_sub(1) as f32 / *total as f32)
            }
            Self::Done => Some(1.0),
            _ => None,
        }
    }
}

/// A refusal's buttons; "Get Minecraft" appears only when the game is missing or not owned.
pub(super) const fn refused_actions(refusal: Refusal, sources: Sources) -> &'static [Action] {
    match (refusal, sources.local_install, sources.account) {
        (Refusal::NotInstalled, ..) => &[Action::GetMinecraft, Action::Retry, Action::Quit],
        // Closing the window quits, so the row stays within three buttons.
        (Refusal::NotOwned, true, _) => &[Action::SignIn, Action::GetMinecraft, Action::Retry],
        (Refusal::NotOwned, false, _) => &[Action::SignIn, Action::GetMinecraft, Action::Quit],
        (Refusal::SignInRequired, true, _) => &[Action::SignIn, Action::Retry, Action::Quit],
        (Refusal::SignInRequired, false, _) => &[Action::SignIn, Action::Quit],
        (Refusal::Unverifiable, _, true) => &[Action::Retry, Action::SignIn, Action::Quit],
        (Refusal::Unverifiable, _, false) => &[Action::Retry, Action::Quit],
    }
}

/// Maps a worker report onto the screen; `meter` turns byte counts into a speed.
pub(super) fn from_status(status: &Status, meter: &mut Meter, now: Instant) -> Screen {
    match status.phase {
        Phase::AwaitingConsent => Screen::Consent,
        Phase::AwaitingOwnership => Screen::Verifying,
        Phase::Downloading => {
            let received = status.downloaded.unwrap_or(0);
            Screen::Downloading {
                received,
                total: status.download_total,
                bytes_per_second: meter.sample(now, received),
            }
        }
        Phase::Running if status.total == 0 => Screen::Starting,
        Phase::Running | Phase::LocalWorlds => Screen::Preparing {
            step: status.step,
            total: status.total,
            label: status.label.clone(),
        },
        Phase::Done => Screen::Done,
        Phase::Failed => Screen::Failed {
            message: status.error.clone().unwrap_or_else(|| status.label.clone()),
        },
    }
}

/// Download speed smoothed over half-second windows.
#[derive(Default)]
pub(super) struct Meter {
    anchor: Option<(Instant, u64)>,
    rate: Option<f64>,
}

impl Meter {
    const WINDOW: Duration = Duration::from_millis(500);

    pub(super) fn sample(&mut self, now: Instant, bytes: u64) -> Option<f64> {
        match self.anchor {
            Some((then, before)) if bytes >= before => {
                let elapsed = now.saturating_duration_since(then);
                if elapsed >= Self::WINDOW {
                    let rate = (bytes - before) as f64 / elapsed.as_secs_f64();
                    self.rate = Some(self.rate.map_or(rate, |old| old * 0.6 + rate * 0.4));
                    self.anchor = Some((now, bytes));
                }
            }
            _ => {
                self.anchor = Some((now, bytes));
                self.rate = None;
            }
        }
        self.rate
    }
}

fn download_detail(received: u64, total: Option<u64>, bytes_per_second: Option<f64>) -> String {
    let mut parts = vec![match total {
        Some(total) if total > 0 => format!(
            "{} of {}",
            megabytes(received as f64),
            megabytes(total as f64)
        ),
        _ => megabytes(received as f64),
    }];
    if let Some(rate) = bytes_per_second {
        parts.push(format!("{}/s", megabytes(rate)));
    }
    if let Some(total) = total.filter(|total| *total > 0) {
        parts.push(format!("{}%", received.saturating_mul(100) / total));
    }
    parts.join(" · ")
}

fn megabytes(bytes: f64) -> String {
    format!("{:.1} MB", bytes / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four buttons overflow the setup panel's action row.
    #[test]
    fn refusals_fit_the_action_row() {
        let refusals = [
            Refusal::NotInstalled,
            Refusal::NotOwned,
            Refusal::SignInRequired,
            Refusal::Unverifiable,
        ];
        for refusal in refusals {
            for (local_install, account) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                let sources = Sources {
                    local_install,
                    account,
                };
                let actions = refused_actions(refusal, sources);
                assert!(
                    (1..=3).contains(&actions.len()),
                    "{refusal:?} {sources:?}: {actions:?}"
                );
            }
        }
    }

    fn status(phase: Phase) -> Status {
        Status::new(phase, 3, 19, "Compiling entity assets")
    }

    #[test]
    fn reports_map_to_screens() {
        let (mut meter, now) = (Meter::default(), Instant::now());
        assert_eq!(
            from_status(&status(Phase::AwaitingConsent), &mut meter, now),
            Screen::Consent
        );
        assert_eq!(
            from_status(&status(Phase::Running), &mut meter, now),
            Screen::Preparing {
                step: 3,
                total: 19,
                label: "Compiling entity assets".into()
            }
        );
        assert_eq!(
            from_status(&Status::starting(), &mut meter, now),
            Screen::Starting
        );
        assert_eq!(
            from_status(&status(Phase::Done), &mut meter, now),
            Screen::Done
        );
        let failed = Status::failed("Failed", "no network");
        assert_eq!(
            from_status(&failed, &mut meter, now),
            Screen::Failed {
                message: "no network".into()
            }
        );
        let screen = from_status(&Status::downloading(50, Some(200)), &mut meter, now);
        assert_eq!(screen.progress(), Some(0.25));
        assert_eq!(screen.body(), "0.0 MB of 0.0 MB · 25%");
    }

    #[test]
    fn download_speed_comes_from_successive_reports() {
        let (mut meter, start) = (Meter::default(), Instant::now());
        assert_eq!(meter.sample(start, 0), None);
        assert_eq!(
            meter.sample(start + Duration::from_millis(100), 1_000_000),
            None
        );
        let rate = meter
            .sample(start + Duration::from_secs(1), 2_000_000)
            .unwrap();
        assert!((rate - 2_000_000.0).abs() < 1.0);
        // A restarted download resets the meter instead of reporting a negative speed.
        assert_eq!(meter.sample(start + Duration::from_secs(2), 10), None);
        assert_eq!(
            download_detail(81_200_000, Some(162_400_000), Some(5_300_000.0)),
            "81.2 MB of 162.4 MB · 5.3 MB/s · 50%"
        );
    }

    #[test]
    fn failure_offers_retry_which_restarts_the_worker() {
        let failed = Screen::Failed {
            message: "timed out".into(),
        };
        assert_eq!(failed.primary(), Some(Action::Retry));
        assert_eq!(
            failed.on_action(Action::Retry),
            (Some(Screen::Starting), Effect::StartWorker)
        );
        assert_eq!(failed.on_action(Action::Quit), (None, Effect::Quit));
        assert_eq!(failed.on_action(Action::Accept), (None, Effect::None));
    }

    #[test]
    fn consent_accepts_or_quits_and_running_screens_only_cancel() {
        assert_eq!(
            Screen::Consent.on_action(Action::Accept),
            (Some(Screen::Starting), Effect::StartWorker)
        );
        assert_eq!(
            Screen::Consent.on_action(Action::ViewEula),
            (None, Effect::OpenEula)
        );
        let running = Screen::Preparing {
            step: 1,
            total: 2,
            label: String::new(),
        };
        assert_eq!(running.primary(), None);
        assert_eq!(running.button_label(Action::Quit), "Cancel");
        assert_eq!(running.on_action(Action::Retry), (None, Effect::None));
        assert_eq!(running.progress(), Some(0.0));
        assert!(Screen::Done.actions().is_empty());
    }

    #[test]
    fn progress_mapping_preserves_download_and_step_boundaries() {
        let download = |received, total| Screen::Downloading {
            received,
            total,
            bytes_per_second: None,
        };
        assert_eq!(download(50, None).progress(), Some(0.0));
        assert_eq!(download(50, Some(0)).progress(), Some(0.0));
        assert_eq!(download(200, Some(100)).progress(), Some(1.0));
        let preparing = Screen::Preparing {
            step: 3,
            total: 4,
            label: String::new(),
        };
        assert_eq!(preparing.progress(), Some(0.5));
        assert_eq!(Screen::Done.progress(), Some(1.0));
        assert_eq!(
            Screen::Failed {
                message: "offline".into()
            }
            .progress(),
            None
        );
        assert_eq!(
            download(50, None).on_action(Action::Quit),
            (None, Effect::Quit)
        );
        assert_eq!(preparing.on_action(Action::Quit), (None, Effect::Quit));
    }

    #[test]
    fn updates_use_update_wording() {
        let downloading = Screen::Downloading {
            received: 0,
            total: None,
            bytes_per_second: None,
        };
        assert_eq!(downloading.title(true), "Updating Minecraft resources…");
        assert_eq!(downloading.title(false), "Downloading Minecraft resources…");
        assert_eq!(
            Screen::Failed {
                message: String::new()
            }
            .title(true),
            "Update failed"
        );
    }
}
