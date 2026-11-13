//! Host support actions: links, dialogs and the diagnostic report.
use launcher::menu::settings_support::*;
use {super::MenuRuntime, launcher::menu::MenuDialog};

impl MenuRuntime {
    /// Opens the authored confirmation, launches its fixed browser destination or asks the host
    /// to save a diagnostic report.
    pub(super) fn activate_support(&mut self, action: SupportAction) {
        match action {
            SupportAction::Dialog(dialog) => {
                self.dialog = Some(MenuDialog::SettingsSupport(dialog))
            }
            SupportAction::Open(link) => {
                self.dialog = None;
                launcher_host::desktop::open_url(link.url());
            }
            SupportAction::SaveReport => {
                self.dialog = None;
                // The row is offered only where the host can save one; a stray activation on
                // another host is logged by the host and changes nothing.
                bevy::log::info!("diagnostic report requested from settings");
                launcher_host::desktop::save_diagnostic_report();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::MenuAction;

    /// The view offers the report row exactly when the host can save a report, and a request
    /// closes an open dialog without leaving the screen.
    #[test]
    fn the_report_row_follows_the_host_and_a_request_closes_dialogs() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".into());
        assert_eq!(
            menu.view().saves_diagnostic_report,
            launcher_host::desktop::saves_diagnostic_reports()
        );
        menu.saves_diagnostic_report = true;
        assert!(menu.view().saves_diagnostic_report);
        menu.dialog = Some(MenuDialog::SettingsSupport(SupportDialog::Help));
        let screen = menu.view().screen;
        menu.activate(MenuAction::SettingsSupport(SupportAction::SaveReport));
        assert_eq!(menu.view().dialog, None);
        assert_eq!(menu.view().screen, screen);
    }
}
