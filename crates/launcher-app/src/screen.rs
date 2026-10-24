//! The launcher window's state: the installation facts it shows, the page it is on and what
//! each button does.

use std::path::Path;

use launcher::{accounts::AccountStore, install_layout::InstallLayout, menu::view::SavedServer};
use update_manifest::channel::Channel;

/// How many recently joined servers the home page offers.
pub const RECENT_SERVERS: usize = 3;
/// How many saved servers the servers page lists; the game's own list holds the rest.
pub const LISTED_SERVERS: usize = 6;

/// The installation page's row for whether game resources are prepared.
pub const RESOURCES_ROW: &str = "Game resources";

/// The signed-in Xbox profile the client will play as.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub gamertag: String,
    /// The cached profile picture, if the client saved one.
    pub picture: Option<String>,
}

/// One saved server and when it was last joined, already worded for display.
#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub name: String,
    pub address: String,
    /// "Joined 3 days ago", or `None` if it was never joined.
    pub joined: Option<String>,
}

/// Facts about this installation the launcher shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    /// The packaged client version.
    pub version: String,
    /// The saved update channel.
    pub channel: Channel,
    /// Whether the game resources are prepared; otherwise the client sets them up on Play.
    pub resources_ready: bool,
    /// Where the client keeps worlds, logs and settings.
    pub data_folder: String,
    /// A development checkout rather than a packaged install.
    pub development: bool,
    pub account: Option<Account>,
    /// Saved servers, most recently joined first.
    pub servers: Vec<Server>,
}

impl Overview {
    /// Reads the overview for `layout` with the update channel saved in its settings; `now` is
    /// the Unix time that join times are worded against.
    pub fn read(layout: &InstallLayout, channel: Channel, now: u64) -> Self {
        let store = AccountStore::new(layout.auth_cache());
        let active = store.active_id().ok().flatten();
        let account = active.and_then(|id| {
            let profile = store.list().ok()?.into_iter().find(|p| p.id == id)?;
            Some(Account {
                gamertag: profile.gamertag,
                picture: profile.picture_path,
            })
        });
        Self {
            version: launcher::PRODUCT_VERSION.to_owned(),
            channel,
            resources_ready: !first_run::needs_preparation(layout),
            data_folder: layout.user_data_root.display().to_string(),
            development: !layout.is_installed(),
            account,
            servers: servers(&layout.server_file(), now),
        }
    }

    /// The channel as shown in the bottom bar and on the installation page.
    pub fn channel_label(&self) -> &'static str {
        if self.development {
            "Development build"
        } else {
            self.channel.label()
        }
    }

    /// The labelled rows of the installation page, in display order.
    pub fn rows(&self) -> [(&'static str, String); 4] {
        [
            ("Version", self.version.clone()),
            ("Update channel", self.channel_label().to_owned()),
            (
                RESOURCES_ROW,
                if self.resources_ready {
                    "Ready".to_owned()
                } else {
                    "Downloaded on first play".to_owned()
                },
            ),
            ("Data folder", self.data_folder.clone()),
        ]
    }

    /// The servers the home page offers, most recently joined first.
    pub fn recent(&self) -> &[Server] {
        &self.servers[..self.servers.len().min(RECENT_SERVERS)]
    }
}

/// Reads the saved servers at `path`, most recently joined first, with join times worded
/// against `now`.
fn servers(path: &Path, now: u64) -> Vec<Server> {
    let mut saved = launcher_host::servers::load_servers(path).servers;
    saved.sort_by_key(|server| std::cmp::Reverse(server.last_joined_unix));
    saved
        .into_iter()
        .map(|server: SavedServer| Server {
            joined: (server.last_joined_unix > 0).then(|| {
                format!(
                    "Joined {}",
                    ago(now.saturating_sub(server.last_joined_unix))
                )
            }),
            name: server.name,
            address: server.address,
        })
        .collect()
}

/// Words an elapsed time in seconds by its largest whole unit.
fn ago(seconds: u64) -> String {
    const DAY: u64 = 24 * 3600;
    const UNITS: [(u64, &str); 6] = [
        (365 * DAY, "year"),
        (30 * DAY, "month"),
        (7 * DAY, "week"),
        (DAY, "day"),
        (3600, "hour"),
        (60, "minute"),
    ];
    UNITS.iter().find(|(unit, _)| seconds >= *unit).map_or_else(
        || "just now".to_owned(),
        |(unit, name)| {
            let count = seconds / unit;
            format!("{count} {name}{} ago", if count == 1 { "" } else { "s" })
        },
    )
}

/// The sidebar's pages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Page {
    Home,
    Servers,
    Installation,
}

impl Page {
    pub const ALL: [Self; 3] = [Self::Home, Self::Servers, Self::Installation];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Servers => "Servers",
            Self::Installation => "Installation",
        }
    }
}

/// Where starting the client stands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Launch {
    Idle,
    /// The client process has started; the launcher closes shortly.
    Starting,
    /// Starting failed; a modal shows the message.
    Failed {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Open(Page),
    Play,
    /// Joins the saved server at this index of [`Overview::servers`].
    Join(usize),
    OpenFolder,
    /// Closes the failure modal.
    Dismiss,
}

/// What pressing a button asks the window to do besides changing state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    None,
    /// Start the client, joining this address if one is given.
    Launch(Option<String>),
    OpenFolder,
}

/// The launcher's navigation and launch state, with its buttons in focus order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Screen {
    pub page: Page,
    pub launch: Launch,
    actions: Vec<Action>,
}

impl Screen {
    /// The home page of a launcher showing `servers` saved servers.
    pub fn new(servers: usize) -> Self {
        Self::at(Page::Home, Launch::Idle, servers)
    }

    /// `page` in launch state `launch`, offering only the buttons that state shows.
    pub fn at(page: Page, launch: Launch, servers: usize) -> Self {
        let actions = match &launch {
            Launch::Starting => Vec::new(),
            Launch::Failed { .. } => vec![Action::Play, Action::Dismiss],
            Launch::Idle => {
                let mut actions: Vec<_> = Page::ALL.into_iter().map(Action::Open).collect();
                let shown = match page {
                    Page::Home => servers.min(RECENT_SERVERS),
                    Page::Servers => servers.min(LISTED_SERVERS),
                    Page::Installation => 0,
                };
                actions.extend((0..shown).map(Action::Join));
                if page == Page::Installation {
                    actions.push(Action::OpenFolder);
                }
                actions.push(Action::Play);
                actions
            }
        };
        Self {
            page,
            launch,
            actions,
        }
    }

    /// Enabled buttons in focus order.
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    /// The next screen (if it changes) and the side effect of pressing `action` on a launcher
    /// showing `overview`.
    pub fn on_action(&self, action: Action, overview: &Overview) -> (Option<Self>, Effect) {
        if !self.actions.contains(&action) {
            return (None, Effect::None);
        }
        let count = overview.servers.len();
        let starting = || Some(Self::at(self.page, Launch::Starting, count));
        match action {
            Action::Open(page) => (Some(Self::at(page, Launch::Idle, count)), Effect::None),
            Action::Play => (starting(), Effect::Launch(None)),
            Action::Join(index) => match overview.servers.get(index) {
                Some(server) => (starting(), Effect::Launch(Some(server.address.clone()))),
                None => (None, Effect::None),
            },
            Action::OpenFolder => (None, Effect::OpenFolder),
            Action::Dismiss => (Some(Self::at(self.page, Launch::Idle, count)), Effect::None),
        }
    }

    /// The screen after the client process started or failed to start.
    pub fn launched(&self, result: Result<(), String>, servers: usize) -> Self {
        let launch = match result {
            Ok(()) => Launch::Starting,
            Err(message) => Launch::Failed { message },
        };
        Self::at(self.page, launch, servers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overview(servers: usize) -> Overview {
        Overview {
            version: "0.1.20".into(),
            channel: Channel::Beta,
            resources_ready: true,
            data_folder: "/data".into(),
            development: false,
            account: None,
            servers: (0..servers)
                .map(|index| Server {
                    name: format!("Server {index}"),
                    address: format!("play{index}.example.net:19132"),
                    joined: None,
                })
                .collect(),
        }
    }

    #[test]
    fn play_and_join_start_the_client_and_a_failure_can_be_retried_or_dismissed() {
        let overview = overview(5);
        let home = Screen::new(5);
        let (starting, effect) = home.on_action(Action::Play, &overview);
        assert_eq!(effect, Effect::Launch(None));
        assert_eq!(starting.unwrap().launch, Launch::Starting);
        let (_, effect) = home.on_action(Action::Join(1), &overview);
        assert_eq!(
            effect,
            Effect::Launch(Some("play1.example.net:19132".into()))
        );
        let failed = home.launched(Err("missing client".into()), 5);
        assert_eq!(failed.actions(), [Action::Play, Action::Dismiss]);
        let (dismissed, _) = failed.on_action(Action::Dismiss, &overview);
        assert_eq!(dismissed.unwrap(), home);
        assert!(home.launched(Ok(()), 5).actions().is_empty());
    }

    #[test]
    fn pages_offer_their_own_buttons_and_play_is_always_last() {
        let overview = overview(5);
        let home = Screen::new(5);
        let joins = |screen: &Screen| {
            screen
                .actions()
                .iter()
                .filter(|action| matches!(action, Action::Join(_)))
                .count()
        };
        assert_eq!(joins(&home), RECENT_SERVERS);
        let servers = home
            .on_action(Action::Open(Page::Servers), &overview)
            .0
            .unwrap();
        assert_eq!(joins(&servers), 5);
        let install = servers
            .on_action(Action::Open(Page::Installation), &overview)
            .0
            .unwrap();
        assert!(install.actions().contains(&Action::OpenFolder));
        assert!(!home.actions().contains(&Action::OpenFolder));
        for screen in [&home, &servers, &install] {
            assert_eq!(screen.actions().last(), Some(&Action::Play));
        }
        assert_eq!(
            install.on_action(Action::Join(0), &overview),
            (None, Effect::None)
        );
    }

    #[test]
    fn rows_describe_channel_and_resources() {
        let rows = overview(0).rows();
        assert_eq!(rows[1].1, Channel::Beta.label());
        assert_eq!(rows[2].1, "Ready");
        let pending = Overview {
            resources_ready: false,
            development: true,
            ..overview(0)
        };
        let rows = pending.rows();
        assert_eq!(rows[1].1, "Development build");
        assert_eq!(rows[2].1, "Downloaded on first play");
    }

    #[test]
    fn saved_servers_list_most_recent_first_with_worded_join_times() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("servers.json");
        let saved = |name: &str, joined| SavedServer {
            name: name.into(),
            address: format!("{name}.example.net:19132"),
            favorite: false,
            last_joined_unix: joined,
        };
        launcher_host::servers::save_servers(
            &path,
            &[saved("never", 0), saved("old", 1_000), saved("new", 99_000)],
        )
        .unwrap();
        let servers = servers(&path, 100_000);
        let names: Vec<_> = servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["new", "old", "never"]);
        assert_eq!(servers[0].joined.as_deref(), Some("Joined 16 minutes ago"));
        assert_eq!(servers[1].joined.as_deref(), Some("Joined 1 day ago"));
        assert_eq!(servers[2].joined, None);
        assert_eq!(ago(30), "just now");
    }
}
