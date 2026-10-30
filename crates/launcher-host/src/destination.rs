//! Typed menu destinations shared by launcher and browser presentation.

use bridge::ConnectTarget;

/// The kind of join `address` starts, for its progress titles.
pub fn join_kind(address: &str, local_world: bool) -> launcher::menu::view::JoinKind {
    use launcher::menu::view::JoinKind;
    match target_for(address) {
        _ if local_world => JoinKind::Local,
        ConnectTarget::Realm(_) => JoinKind::Realm,
        // Friend worlds and experiences use the external-server title until vanilla's is confirmed.
        ConnectTarget::Server(_) | ConnectTarget::Friend(_) | ConnectTarget::Gathering(_) => {
            JoinKind::External
        }
    }
}

/// Classifies service destinations while preserving the server input for transport selection.
pub fn target_for(address: &str) -> ConnectTarget {
    let address = address.trim();
    if let Some(id) = address.strip_prefix(launcher::menu::EXPERIENCE_ADDRESS_PREFIX) {
        return ConnectTarget::Gathering(id.to_owned());
    }
    if let Some(id) = address.strip_prefix(bridge::contract::REALM_TARGET_PREFIX) {
        return ConnectTarget::Realm(id.to_owned());
    }
    if let Some(xuid) = address.strip_prefix(launcher::menu::FRIEND_ADDRESS_PREFIX) {
        return ConnectTarget::Friend(xuid.to_owned());
    }
    ConnectTarget::Server(address.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HTTP signaling URLs retain their authority, explicit port, and scheme casing.
    #[test]
    fn http_urls_preserve_their_original_authority() {
        for address in [
            "HTTPS://example.test",
            "hTtP://example.test:19133",
            "https://[::1]:19134",
        ] {
            assert_eq!(target_for(address), ConnectTarget::Server(address.into()));
        }
    }

    #[test]
    fn menu_addresses_map_to_connect_targets() {
        assert_eq!(
            target_for(&format!("{}42", bridge::contract::REALM_TARGET_PREFIX)),
            ConnectTarget::Realm("42".into())
        );
        assert_eq!(
            target_for(&format!(
                "{}5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f",
                bridge::contract::GATHERING_TARGET_PREFIX
            )),
            ConnectTarget::Gathering("5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f".into())
        );
        assert_eq!(
            target_for(&format!("{}2535", bridge::contract::FRIEND_TARGET_PREFIX)),
            ConnectTarget::Friend("2535".into())
        );
        assert_eq!(
            target_for("play.example.net:19133"),
            ConnectTarget::Server("play.example.net:19133".into())
        );
        assert_eq!(
            target_for("play.example.net"),
            ConnectTarget::Server("play.example.net".into())
        );
        assert_eq!(
            target_for("[::1]:19134"),
            ConnectTarget::Server("[::1]:19134".into())
        );
        assert_eq!(target_for("::1"), ConnectTarget::Server("::1".into()));
    }
}
