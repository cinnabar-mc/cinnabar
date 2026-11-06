//! What a host without a setup window shows while setup runs, derived from the setup window's own
//! screen state so both describe each report the same way.

use std::time::Instant;

use serde::Serialize;

use super::{
    screen::{Action, Meter, Screen, from_status},
    status::{Phase, Status},
};

/// One update for a progress display.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Progress {
    /// The work underway, as a short heading.
    pub title: String,
    /// Byte counts or the current step; empty when there is nothing to add.
    pub detail: String,
    /// Fraction done, or `None` when the work has no measurable extent.
    pub fraction: Option<f32>,
    /// The label of the control that abandons setup, while setup can still be abandoned.
    pub cancel: Option<String>,
}

impl Progress {
    /// Describes `status`, or returns `None` for reports the host presents as its own prompts:
    /// the consent request and the final failure.
    pub(super) fn new(
        status: &Status,
        meter: &mut Meter,
        now: Instant,
        updating: bool,
    ) -> Option<Self> {
        let screen = from_status(status, meter, now);
        if matches!(screen, Screen::Consent | Screen::Failed { .. }) {
            return None;
        }
        let cancel = screen
            .actions()
            .contains(&Action::Quit)
            .then(|| screen.button_label(Action::Quit).to_owned());
        // Local-world lines carry their own wording and have no step count. Their counts follow
        // the colon and change constantly, so they belong in the detail, not the heading.
        let (title, detail, fraction) = if status.phase == Phase::LocalWorlds {
            let (title, detail) = status
                .label
                .split_once(": ")
                .unwrap_or((status.label.as_str(), ""));
            (title.to_owned(), detail.to_owned(), None)
        } else {
            (
                screen.title(updating).to_owned(),
                screen.body(),
                screen.progress(),
            )
        };
        Some(Self {
            title,
            detail,
            fraction,
            cancel,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(status: &Status, updating: bool) -> Option<Progress> {
        Progress::new(status, &mut Meter::default(), Instant::now(), updating)
    }

    #[test]
    fn prompts_are_left_to_the_host() {
        assert_eq!(progress(&Status::awaiting_consent(), false), None);
        assert_eq!(progress(&Status::failed("Failed", "offline"), false), None);
    }

    #[test]
    fn a_download_reports_its_bytes_and_fraction() {
        let update = progress(&Status::downloading(50_000_000, Some(200_000_000)), false).unwrap();
        assert_eq!(update.title, "Downloading Minecraft resources…");
        assert_eq!(update.detail, "50.0 MB of 200.0 MB · 25%");
        assert_eq!(update.fraction, Some(0.25));
        assert_eq!(update.cancel.as_deref(), Some("Cancel"));
    }

    #[test]
    fn an_update_uses_update_wording() {
        let update = progress(&Status::downloading(0, None), true).unwrap();
        assert_eq!(update.title, "Updating Minecraft resources…");
    }

    #[test]
    fn asset_steps_report_their_position() {
        let status = Status::new(Phase::Running, 3, 5, "Compiling entity assets");
        let update = progress(&status, false).unwrap();
        assert_eq!(update.detail, "Step 3 of 5: Compiling entity assets");
        assert_eq!(update.fraction, Some(0.4));
    }

    #[test]
    fn local_world_lines_stand_alone_without_a_measured_extent() {
        let line = "Downloading local world server: 4 / 8 MB";
        let status = Status::new(Phase::LocalWorlds, 1, 1, line);
        let update = progress(&status, false).unwrap();
        assert_eq!(update.title, "Downloading local world server");
        assert_eq!(update.detail, "4 / 8 MB");
        assert_eq!(update.fraction, None);
        assert_eq!(update.cancel.as_deref(), Some("Cancel"));
        let plain = Status::new(Phase::LocalWorlds, 1, 1, "Preparing local worlds");
        let update = progress(&plain, false).unwrap();
        assert_eq!(update.title, "Preparing local worlds");
        assert_eq!(update.detail, "");
    }

    #[test]
    fn readiness_cannot_be_cancelled() {
        let update = progress(&Status::ready(), false).unwrap();
        assert_eq!(update.fraction, Some(1.0));
        assert_eq!(update.cancel, None);
    }

    #[test]
    fn updates_serialize_for_the_android_activity() {
        let update = progress(&Status::ready(), false).unwrap();
        let value = serde_json::to_value(&update).unwrap();
        assert_eq!(value["fraction"], 1.0);
        assert!(value["cancel"].is_null());
        let local = progress(&Status::new(Phase::LocalWorlds, 1, 1, "x"), false).unwrap();
        assert!(serde_json::to_value(&local).unwrap()["fraction"].is_null());
    }
}
