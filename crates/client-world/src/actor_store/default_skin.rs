//! Skins that carry no pixels and ask for the client's own default skin.
use super::{ActorStore, PlayerSkin};
use protocol::PlayerSkinUnavailable;
use render_api::{CLASSIC_SKIN_SIDE, StandardSkin};

impl ActorStore {
    /// Sets whether the session is a Java Edition one, whose players without a skin wear the
    /// default skin in the 64x32 layout with any cape they have.
    pub(crate) fn set_java_session(&mut self, java: bool) {
        self.java_session = java;
    }

    /// Resolves a skin without pixels ([`protocol::is_client_default_skin`]) before it is kept.
    /// On a Java Edition session it becomes the installed default skin in the 64x32 layout
    /// ([`render_model::legacy_default_skin_rgba8`]), keeping its cape and model, as the 1.7.10
    /// client draws a player's cape over its default Steve. Elsewhere, and before a default
    /// texture is installed, it stays unusable: players draw the default skin without a cape
    /// and a skin update without pixels is ignored. Other skins pass through.
    pub(super) fn resolve_default_skin(&self, skin: PlayerSkin) -> PlayerSkin {
        if !protocol::is_client_default_skin(&skin) {
            return skin;
        }
        let PlayerSkin::Standard(requested) = skin else {
            unreachable!("a default skin request is a standard skin")
        };
        match render_model::legacy_default_skin_rgba8().filter(|_| self.java_session) {
            Some(rgba8) => PlayerSkin::Standard(StandardSkin {
                width: CLASSIC_SKIN_SIDE as u32,
                height: CLASSIC_SKIN_SIDE as u32,
                rgba8,
                ..requested
            }),
            None => PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ActorApplyResult;
    use super::*;
    use protocol::{ActorEvent, PlayerListEntry, PlayerListUpdateEvent};
    use render_api::CapeImage;
    use std::sync::Arc;

    const UUID: [u8; 16] = [3; 16];

    /// A skin without pixels with a 64x32 cape, as a Java session sends a cape over the default
    /// skin.
    fn default_with_cape() -> PlayerSkin {
        PlayerSkin::Standard(StandardSkin {
            width: 0,
            height: 0,
            rgba8: Vec::new().into(),
            cape: Some(CapeImage {
                width: 64,
                height: 32,
                rgba8: Arc::from(vec![5u8; 64 * 32 * 4]),
            }),
            geometry: None,
        })
    }

    /// Installs a default texture; the installed one is process-wide, so tests compare with
    /// whichever was installed first.
    fn installed_legacy_default() -> render_api::SkinRgba8 {
        render_model::install_default_player_skin(Arc::from(vec![9u8; 64 * 64 * 4]));
        render_model::legacy_default_skin_rgba8().expect("a default texture is installed")
    }

    fn list(store: &mut ActorStore, skin: PlayerSkin) -> ActorApplyResult {
        store.apply(
            1,
            1,
            ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Add {
                    uuid: UUID,
                    unique_id: 1,
                    username: "Alex".into(),
                    verified: true,
                    xuid: "".into(),
                    skin,
                }]),
            }),
        )
    }

    #[test]
    fn a_java_player_without_a_skin_wears_the_legacy_default_with_its_cape() {
        let legacy = installed_legacy_default();
        let mut store = ActorStore::new(1, 0);
        store.set_java_session(true);
        assert_eq!(
            list(&mut store, default_with_cape()),
            ActorApplyResult::Updated
        );
        let PlayerSkin::Standard(skin) = &store.players[&UUID].skin else {
            panic!("the default skin is usable");
        };
        assert_eq!((skin.width, skin.height), (64, 64));
        assert_eq!(skin.rgba8, legacy);
        assert_eq!(
            skin.cape.as_ref().map(|cape| (cape.width, cape.height)),
            Some((64, 32))
        );

        // A later update that only carries a cape is applied too.
        assert_eq!(
            store.apply_skin_update(UUID, default_with_cape()),
            ActorApplyResult::Updated
        );
    }

    #[test]
    fn a_bedrock_player_without_a_skin_stays_unusable() {
        installed_legacy_default();
        let mut store = ActorStore::new(1, 0);
        assert_eq!(
            list(&mut store, default_with_cape()),
            ActorApplyResult::Updated
        );
        assert!(matches!(
            store.players[&UUID].skin,
            PlayerSkin::Unavailable(_)
        ));
        assert_eq!(
            store.apply_skin_update(UUID, default_with_cape()),
            ActorApplyResult::CapacityRejected
        );
    }
}
