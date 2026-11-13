//! Account and Help Center routes use the platform browser and native dialogs; a diagnostic
//! report is saved by the host.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportLink {
    Help,
    Attribution,
    LicensedContent,
    Gamertag,
    Account,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportDialog {
    Help,
    FontLicense,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportAction {
    Open(SupportLink),
    Dialog(SupportDialog),
    /// Saves a report of the running session where the user can share it; offered only while
    /// `MenuView::saves_diagnostic_report` is set.
    SaveReport,
}

/// The General settings row that saves a diagnostic report.
pub const SAVE_REPORT_TITLE: &str = "Save diagnostic report";
/// What the report holds and where it goes, shown under [`SAVE_REPORT_TITLE`].
pub const SAVE_REPORT_DESCRIPTION: &str = "Saves a ZIP with this session's log and device details to Downloads. The log can name servers and players. Nothing is uploaded.";
/// The button of the report row.
pub const SAVE_REPORT_LABEL: &str = "Save";

impl SupportLink {
    /// Fixed destinations from general_section.json and vanilla's feedback link.
    pub fn url(self) -> &'static str {
        match self {
            Self::Help => "https://aka.ms/MCHelp",
            Self::Attribution => "https://www.minecraft.net/attribution/?hideChrome",
            Self::LicensedContent => "https://www.minecraft.net/licensed-content/?hideChrome",
            Self::Gamertag => "https://social.xbox.com/changegamertag",
            Self::Account => "https://account.xbox.com/Settings",
        }
    }
}

/// Attribution shown for the font bundled with the client.
pub fn font_licenses() -> String {
    "Cinnangles Sans\n\nBundled with Cinnabar.".to_owned()
}
