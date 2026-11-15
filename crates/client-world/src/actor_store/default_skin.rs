//! Skins that carry no pixels and ask for the client's own default skin.
//!
//! Only Java Edition sessions decode such requests ([`protocol::EmptySkin::ClientDefault`]);
//! they are drawn as 1.7.10 draws a player without a skin: the installed default texture in
//! the 64x32 layout, with any cape over it.
use super::{ActorApplyResult, ActorStore, PlayerSkin};
use protocol::PlayerSkinUnavailable;
use render_api::{CLASSIC_SKIN_SIDE, SkinRgba8, StandardSkin};

/// Supplies the default skin in the 64x32 layout, or `None` until a default texture is
/// installed.
pub(super) type LegacyDefaultSkin = fn() -> Option<SkinRgba8>;

impl ActorStore {
    /// Resolves a skin without pixels ([`protocol::is_client_default_skin`]) for `uuid` before
    /// it is kept: it becomes the installed default skin in the 64x32 layout, keeping its cape
    /// and model. Until a default texture is installed it stays unusable, which draws the
    /// default skin without a cape, and is kept to be applied once one is
    /// ([`Self::resolve_pending_default_skins`]). Other skins pass through and drop any such
    /// pending request.
    pub(super) fn resolve_default_skin(&mut self, uuid: [u8; 16], skin: PlayerSkin) -> PlayerSkin {
        if !protocol::is_client_default_skin(&skin) {
            self.pending_default_skins.remove(&uuid);
            return skin;
        }
        let Some(rgba8) = (self.legacy_default_skin)() else {
            self.pending_default_skins.insert(uuid, skin);
            return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
        };
        self.pending_default_skins.remove(&uuid);
        let PlayerSkin::Standard(requested) = skin else {
            unreachable!("a default skin request is a standard skin")
        };
        PlayerSkin::Standard(StandardSkin {
            width: CLASSIC_SKIN_SIDE as u32,
            height: CLASSIC_SKIN_SIDE as u32,
            rgba8,
            ..requested
        })
    }

    /// Applies the default skin requests kept while no default texture was installed, once one
    /// is. Returns whether a player's skin changed, so the roster can be redrawn.
    pub(crate) fn resolve_pending_default_skins(&mut self) -> bool {
        if self.pending_default_skins.is_empty() || (self.legacy_default_skin)().is_none() {
            return false;
        }
        let mut changed = false;
        for (uuid, request) in std::mem::take(&mut self.pending_default_skins) {
            changed |= self.apply_skin_update(uuid, request) == ActorApplyResult::Updated;
        }
        changed
    }

    /// Replaces where the default skin comes from, for tests that control when it is installed.
    #[cfg(test)]
    pub(super) fn set_legacy_default_skin(&mut self, source: LegacyDefaultSkin) {
        self.legacy_default_skin = source;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ActorEvent, PlayerListEntry, PlayerListUpdateEvent};
    use render_api::CapeImage;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

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

    fn legacy_pixels() -> SkinRgba8 {
        static PIXELS: std::sync::OnceLock<SkinRgba8> = std::sync::OnceLock::new();
        PIXELS.get_or_init(|| vec![9u8; 64 * 64 * 4].into()).clone()
    }

    fn installed() -> Option<SkinRgba8> {
        Some(legacy_pixels())
    }

    fn list(store: &mut ActorStore, skin: PlayerSkin) -> ActorApplyResult {
        list_as(store, UUID, 1, skin)
    }

    /// Lists player `uuid` with entity unique id `unique_id`, at the next sequence.
    fn list_as(
        store: &mut ActorStore,
        uuid: [u8; 16],
        unique_id: i64,
        skin: PlayerSkin,
    ) -> ActorApplyResult {
        let sequence = store.latest_sequence() + 1;
        store.apply(
            1,
            sequence,
            ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Add {
                    uuid,
                    unique_id,
                    username: "Alex".into(),
                    verified: true,
                    xuid: "".into(),
                    skin,
                }]),
            }),
        )
    }

    /// The kept skin is the legacy default with the 64x32 cape.
    fn assert_default_with_cape(store: &ActorStore) {
        let PlayerSkin::Standard(skin) = &store.players[&UUID].skin else {
            panic!("the default skin is usable");
        };
        assert_eq!((skin.width, skin.height), (64, 64));
        assert_eq!(skin.rgba8, legacy_pixels());
        assert_eq!(
            skin.cape.as_ref().map(|cape| (cape.width, cape.height)),
            Some((64, 32))
        );
    }

    #[test]
    fn a_player_without_a_skin_wears_the_legacy_default_with_its_cape() {
        let mut store = ActorStore::new(1, 0);
        store.set_legacy_default_skin(installed);
        assert_eq!(
            list(&mut store, default_with_cape()),
            ActorApplyResult::Updated
        );
        assert_default_with_cape(&store);

        // A later update that only carries a cape is applied too.
        assert_eq!(
            store.apply_skin_update(UUID, default_with_cape()),
            ActorApplyResult::Updated
        );
        assert_default_with_cape(&store);
    }

    /// A request that arrives before the default texture is installed draws the plain default
    /// until then, and gets its cape once the texture is installed.
    #[test]
    fn a_request_before_the_default_is_installed_resolves_once_it_is() {
        static INSTALLED: AtomicBool = AtomicBool::new(false);
        fn later() -> Option<SkinRgba8> {
            INSTALLED.load(Ordering::Relaxed).then(legacy_pixels)
        }
        let mut store = ActorStore::new(1, 0);
        store.set_legacy_default_skin(later);
        assert_eq!(
            list(&mut store, default_with_cape()),
            ActorApplyResult::Updated
        );
        assert!(matches!(
            store.players[&UUID].skin,
            PlayerSkin::Unavailable(_)
        ));
        assert!(
            !store.resolve_pending_default_skins(),
            "nothing to apply yet"
        );

        INSTALLED.store(true, Ordering::Relaxed);
        assert!(store.resolve_pending_default_skins());
        assert_default_with_cape(&store);
        assert!(!store.resolve_pending_default_skins(), "applied once");
    }

    /// The default texture never arrives on a client without it, so requests must not outlive
    /// the players they were for: unlisted players, players the roster refused and unknown
    /// players keep none.
    #[test]
    fn pending_requests_end_with_their_players() {
        fn never() -> Option<SkinRgba8> {
            None
        }
        let mut store = ActorStore::with_limits(1, 0, 4, 1, 1 << 20);
        store.set_legacy_default_skin(never);

        assert_eq!(
            list(&mut store, default_with_cape()),
            ActorApplyResult::Updated
        );
        assert_eq!(store.pending_default_skins.len(), 1);
        let sequence = store.latest_sequence() + 1;
        store.apply(
            1,
            sequence,
            ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Remove { uuid: UUID }]),
            }),
        );
        assert!(
            store.pending_default_skins.is_empty(),
            "an unlisted player keeps a request"
        );

        // The roster holds one player, so a second is refused.
        list(&mut store, default_with_cape());
        assert_eq!(
            list_as(&mut store, [4; 16], 2, default_with_cape()),
            ActorApplyResult::CapacityRejected
        );
        assert!(
            !store.pending_default_skins.contains_key(&[4; 16]),
            "a refused player keeps a request"
        );

        assert_eq!(
            store.apply_skin_update([5; 16], default_with_cape()),
            ActorApplyResult::MissingActor
        );
        assert!(
            !store.pending_default_skins.contains_key(&[5; 16]),
            "an unknown player keeps a request"
        );
    }

    /// The listed player's actor, which keeps its appearance after the roster entry goes.
    fn spawn_listed_player(store: &mut ActorStore) {
        let sequence = store.latest_sequence() + 1;
        store.apply(
            1,
            sequence,
            ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 2,
                kind: protocol::ActorKind::Player {
                    uuid: UUID,
                    username: "Alex".into(),
                },
                position: [0.0, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            }),
        );
    }

    fn unlist(store: &mut ActorStore) {
        let sequence = store.latest_sequence() + 1;
        store.apply(
            1,
            sequence,
            ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Remove { uuid: UUID }]),
            }),
        );
    }

    /// A player whose actor outlives its roster entry still shows its appearance, so its
    /// request waits with it and gets the cape once the default arrives; the request goes when
    /// the actor does.
    #[test]
    fn a_request_waits_while_an_unlisted_actor_shows_it() {
        static INSTALLED: AtomicBool = AtomicBool::new(false);
        fn later() -> Option<SkinRgba8> {
            INSTALLED.load(Ordering::Relaxed).then(legacy_pixels)
        }
        let mut store = ActorStore::new(1, 0);
        store.set_legacy_default_skin(later);
        list(&mut store, default_with_cape());
        spawn_listed_player(&mut store);
        unlist(&mut store);
        assert!(store.unlisted_players.contains_key(&UUID));
        assert!(store.pending_default_skins.contains_key(&UUID));

        INSTALLED.store(true, Ordering::Relaxed);
        assert!(store.resolve_pending_default_skins());
        let PlayerSkin::Standard(skin) = &store.unlisted_players[&UUID].skin else {
            panic!("the unlisted appearance gets the default skin");
        };
        assert!(skin.cape.is_some(), "and its cape");

        // Once the actor goes, nothing keeps a request for it.
        INSTALLED.store(false, Ordering::Relaxed);
        list(&mut store, default_with_cape());
        unlist(&mut store);
        let sequence = store.latest_sequence() + 1;
        store.apply(
            1,
            sequence,
            ActorEvent::Remove(protocol::ActorRemoveEvent {
                dimension: 0,
                unique_id: 1,
            }),
        );
        assert!(!store.unlisted_players.contains_key(&UUID));
        assert!(store.pending_default_skins.is_empty());
    }

    /// A real skin that replaces a pending request is not overwritten when the default
    /// texture arrives.
    #[test]
    fn a_later_skin_drops_the_pending_request() {
        static INSTALLED: AtomicBool = AtomicBool::new(false);
        fn later() -> Option<SkinRgba8> {
            INSTALLED.load(Ordering::Relaxed).then(legacy_pixels)
        }
        let mut store = ActorStore::new(1, 0);
        store.set_legacy_default_skin(later);
        list(&mut store, default_with_cape());
        let real = PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![1u8; 64 * 64 * 4].into(),
            cape: None,
            geometry: None,
        });
        assert_eq!(
            store.apply_skin_update(UUID, real.clone()),
            ActorApplyResult::Updated
        );
        INSTALLED.store(true, Ordering::Relaxed);
        assert!(!store.resolve_pending_default_skins());
        assert_eq!(store.players[&UUID].skin, real);
    }
}
