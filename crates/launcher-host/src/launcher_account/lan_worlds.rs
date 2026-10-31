//! Polls `lan_worlds.v1` while the Worlds tab shows, so the core browses the local network only
//! then, and maps its rows to menu cards.

use std::{path::Path, sync::Mutex, time::Duration};

use bridge::LanWorld;
use crossbeam_channel::Receiver;
use launcher::menu::view::MenuLanCard;

use super::{Snapshot, publish};

/// How often the shown list is refreshed; LAN hosts answer every one to two seconds.
pub(super) const LAN_INTERVAL: Duration = Duration::from_secs(1);
/// How long the worker sleeps while the tab is hidden before checking again.
const HIDDEN_WAIT: Duration = Duration::from_secs(60);

/// Lists LAN worlds every `LAN_INTERVAL` while the snapshot says the tab shows; `wake` starts a
/// round as soon as it does, and `stop` ends the worker.
pub(super) fn poll(
    socket_dir: &Path,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    wake: &Receiver<()>,
) {
    let Some(runtime) = super::runtime() else {
        return;
    };
    loop {
        let visible = shared
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .lan_visible;
        if visible {
            match runtime.block_on(bridge::list_lan_worlds(socket_dir)) {
                Ok(worlds) => publish(shared, |snapshot| {
                    if snapshot.lan_visible {
                        snapshot.lan_worlds = Some(worlds);
                    }
                }),
                Err(error) => tracing::debug!(%error, "LAN worlds unavailable"),
            }
        }
        let wait = if visible { LAN_INTERVAL } else { HIDDEN_WAIT };
        if !super::feeds::wait_changes(stop, wake, wait) {
            return;
        }
    }
}

/// The Worlds tab card for a LAN world, its player count worded as friends' worlds are.
pub(super) fn lan_card(world: &LanWorld) -> MenuLanCard {
    let players = if world.max_players > 0 {
        format!("{}/{} players", world.players, world.max_players)
    } else {
        format!("{} players", world.players)
    };
    MenuLanCard {
        level_name: world.level_name.clone(),
        server_name: world.server_name.clone(),
        game_mode: world.game_mode.clone(),
        hardcore: world.hardcore,
        players,
        address: world.address.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row keeps the address that joins it and words its players like a friend's world.
    #[test]
    fn lan_card_words_players_and_keeps_the_join_address() {
        let world = LanWorld {
            address: "lan/12345".into(),
            transport: "nethernet".into(),
            level_name: "LAN World".into(),
            server_name: "Host".into(),
            game_mode: "Creative".into(),
            hardcore: false,
            players: 1,
            max_players: 8,
            version: "1.26.60".into(),
        };
        let card = lan_card(&world);
        assert_eq!(card.players, "1/8 players");
        assert_eq!(card.address, "lan/12345");
        assert_eq!(card.level_name, "LAN World");
        assert_eq!(card.server_name, "Host");
        let unbounded = lan_card(&LanWorld {
            max_players: 0,
            ..world
        });
        assert_eq!(unbounded.players, "1 players");
    }
}
