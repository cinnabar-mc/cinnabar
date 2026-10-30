//! What the menu knows about a destination that Discord's card shows: art and player limits.

use bridge::ConnectTarget;

use super::{MenuRuntime, target_for};
use launcher_host::destination::same_presentation_destination;

impl MenuRuntime {
    /// The featured server `address` joins, as Discord's corner art, when it has a logo URL.
    pub fn featured_badge(&self, address: &str) -> Option<rich_presence::Badge> {
        let server = self
            .featured
            .iter()
            .find(|server| same_presentation_destination(&server.address, address))?;
        let image_url = &self.feeds.details.get(&server.address)?.logo_url;
        (!image_url.is_empty()).then(|| rich_presence::Badge {
            image_url: image_url.clone(),
            name: server.name.clone(),
        })
    }

    /// The player limit the menu last saw for `address`: a server's ping, a Realm's slots or a
    /// friend world's published limit.
    pub fn destination_max_players(&self, address: &str) -> Option<u32> {
        let target = target_for(address);
        let max = match &target {
            ConnectTarget::Server(_) => self
                .feeds
                .pings
                .iter()
                .find(|(pinged, ping)| {
                    ping.online && same_presentation_destination(pinged, address)
                })
                .map(|(_, ping)| ping.max_players),
            ConnectTarget::Realm(_) => self
                .realms
                .iter()
                .find(|realm| {
                    [&realm.target, &realm.address]
                        .iter()
                        .any(|candidate| !candidate.is_empty() && target_for(candidate) == target)
                })
                .map(|realm| realm.max_players),
            ConnectTarget::Friend(xuid) => self
                .friends
                .iter()
                .find(|friend| &friend.xuid == xuid)
                .map(|friend| friend.max_members),
            ConnectTarget::Gathering(_) => None,
        };
        max.filter(|max| *max > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::{MenuFriendCard, MenuRealmCard, PingInfo};

    #[test]
    fn limits_come_from_whatever_the_menu_saw_for_the_same_destination() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.feeds.pings.insert(
            "play.example.net".into(),
            PingInfo {
                online: true,
                players: 3,
                max_players: 100,
                ping_ms: 20,
                motd: String::new(),
            },
        );
        menu.feeds.pings.insert(
            "down.example.net".into(),
            PingInfo {
                online: false,
                players: 0,
                max_players: 50,
                ping_ms: 0,
                motd: String::new(),
            },
        );
        menu.realms.push(MenuRealmCard {
            name: "Realm".into(),
            state: "open".into(),
            target: "realm_id/7".into(),
            address: String::new(),
            owner: String::new(),
            online_players: 1,
            max_players: 10,
            days_left: 0,
            expired: false,
            member: true,
        });
        menu.friends.push(MenuFriendCard {
            gamertag: "Alex".into(),
            world_name: "Base".into(),
            members: String::new(),
            xuid: "2535".into(),
            max_members: 8,
        });
        let friend = format!("{}2535", launcher::menu::FRIEND_ADDRESS_PREFIX);
        assert_eq!(
            menu.destination_max_players("play.example.net:19132"),
            Some(100)
        );
        for (listed, aliases) in [
            (
                "::1",
                vec![
                    format!("[::1]:{}", launcher::menu::DEFAULT_PORT),
                    format!("https://[::1]:{}", launcher::menu::DEFAULT_PORT),
                ],
            ),
            (
                "https://secure.example",
                vec!["https://secure.example:443".into()],
            ),
            (
                "http://plain.example",
                vec!["http://plain.example:80".into()],
            ),
        ] {
            menu.feeds.pings.insert(
                listed.into(),
                PingInfo {
                    online: true,
                    players: 3,
                    max_players: 200,
                    ping_ms: 20,
                    motd: String::new(),
                },
            );
            for alias in aliases {
                assert_eq!(menu.destination_max_players(&alias), Some(200), "{alias}");
            }
        }
        for distinct in [
            "play.example.net:19133",
            "https://secure.example:444",
            "http://secure.example:443",
            "http://plain.example:81",
            "https://plain.example:80",
            "[::1]:19133",
            "https://user@secure.example",
            "https://secure.example/path",
        ] {
            assert_eq!(menu.destination_max_players(distinct), None, "{distinct}");
        }
        assert_eq!(
            menu.destination_max_players("down.example.net"),
            None,
            "offline ping"
        );
        assert_eq!(menu.destination_max_players("realm_id/7"), Some(10));
        assert_eq!(menu.destination_max_players(&friend), Some(8));
        assert_eq!(menu.destination_max_players("unknown.example.net"), None);
    }
}
