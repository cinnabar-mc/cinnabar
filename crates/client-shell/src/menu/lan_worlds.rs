//! The Worlds tab's LAN worlds: the core browses the local network only while the tab shows, and
//! a row joins over the transport that found it.

use launcher::menu::{MenuAction, MenuScreen};
use launcher_host::account_control::AccountControl;

use super::MenuRuntime;

impl MenuRuntime {
    /// Whether the Worlds tab, the only place LAN worlds are listed, is on screen.
    fn lan_worlds_visible(&self) -> bool {
        self.visible && self.screen == MenuScreen::Play && !self.is_connecting()
    }

    /// Keeps the core browsing while the Worlds tab shows and takes its latest list; rows are
    /// cleared while the tab is hidden, so a return never shows worlds that have gone.
    pub(super) fn sync_lan_worlds(&mut self, control: &mut dyn AccountControl) {
        let visible = self.lan_worlds_visible();
        control.set_lan_worlds_visible(visible);
        if !visible {
            self.lan_worlds.replace(Vec::new());
            return;
        }
        if let Some(worlds) = control.lan_worlds() {
            self.lan_worlds.replace(worlds);
        }
    }

    /// Joins a listed LAN world; true when `action` named one.
    pub(super) fn lan_action(&mut self, action: MenuAction) -> bool {
        let MenuAction::PlayLan(index) = action else {
            return false;
        };
        if let Some(address) = self
            .lan_worlds
            .get(index)
            .map(|world| world.address.clone())
        {
            self.request_connect(address);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use launcher::menu::{
        MenuAction, MenuScreen,
        auth::AuthState,
        view::{MenuFriendCard, MenuLanCard, MenuRealmCard},
    };
    use launcher_host::account_control::{AccountControl, AccountEvent};

    use super::MenuRuntime;

    /// A core that lists one LAN world while the tab shows and records what the menu asked for.
    #[derive(Default)]
    struct Lan {
        visible: Vec<bool>,
        worlds: Option<Vec<MenuLanCard>>,
    }

    impl AccountControl for Lan {
        fn account_status(&mut self) -> Option<AuthState> {
            None
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
        fn set_lan_worlds_visible(&mut self, visible: bool) {
            self.visible.push(visible);
        }
        fn lan_worlds(&mut self) -> Option<Vec<MenuLanCard>> {
            self.worlds.take()
        }
    }

    fn world(address: &str) -> MenuLanCard {
        MenuLanCard {
            level_name: "LAN World".into(),
            server_name: "Host".into(),
            game_mode: "Creative".into(),
            players: "1/8 players".into(),
            address: address.into(),
            ..Default::default()
        }
    }

    /// The Worlds tab browses and lists LAN worlds, a row joins its address, and leaving the
    /// tab stops browsing and clears the rows.
    #[test]
    fn worlds_tab_lists_and_joins_lan_worlds_only_while_shown() {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.enter(MenuScreen::Play);
        let mut control = Lan {
            worlds: Some(vec![
                world("lan/12345"),
                world("lan/raknet/192.168.1.4:19132"),
            ]),
            ..Default::default()
        };
        menu.sync_account_control(&mut control);
        assert_eq!(control.visible, [true]);
        let view = menu.view();
        assert_eq!(view.lan_worlds.len(), 2);
        assert_eq!(view.lan_worlds[0].level_name, "LAN World");

        menu.activate(MenuAction::PlayLan(1));
        let join = menu.take_join_intent().expect("the row queues a join");
        assert_eq!(join.address, "lan/raknet/192.168.1.4:19132");
        assert!(!join.local_world);

        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.enter(MenuScreen::Play);
        let mut control = Lan {
            worlds: Some(vec![world("lan/12345")]),
            ..Default::default()
        };
        menu.sync_account_control(&mut control);
        menu.enter(MenuScreen::Servers);
        menu.sync_account_control(&mut control);
        assert_eq!(control.visible, [true, false]);
        assert!(menu.view().lan_worlds.is_empty());
    }
}
