use bevy::prelude::Resource;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
mod disk;
#[cfg(not(target_arch = "wasm32"))]
pub use disk::CLIENT_BLOB_DISK_CAP_BYTES;

/// The verified blob cache shared by every network session in this process.
#[derive(Debug, Clone, Resource)]
pub struct ClientBlobCacheOwner(Option<protocol::ClientBlobCache>);

impl Default for ClientBlobCacheOwner {
    fn default() -> Self {
        Self(Some(protocol::ClientBlobCache::default()))
    }
}

impl ClientBlobCacheOwner {
    /// Starts background disk storage and warming without delaying client startup.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn persistent(path: PathBuf) -> Self {
        Self(Some(disk::start(
            path,
            protocol::BlobCacheLimits::default(),
        )))
    }

    /// Returns the verified cache when this connection advertised cache support.
    pub fn cache(&self) -> Option<protocol::ClientBlobCache> {
        self.0.clone()
    }

    /// Captures a connection's cache mode before its core starts; later selections cannot change it.
    pub fn for_session(&self, enabled: bool) -> Self {
        Self(self.0.clone().filter(|_| enabled))
    }

    /// Whether this connection's core may advertise upstream client-cache support.
    /// This must match the optional cache passed to the downstream login.
    pub fn enables_upstream_client_cache(&self) -> bool {
        self.0.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_advertisement_and_resolver_share_the_captured_cache_mode() {
        let owner = ClientBlobCacheOwner::default();
        let ordinary = owner.for_session(true);
        let packets = owner.for_session(false);
        assert!(ordinary.enables_upstream_client_cache());
        assert!(ordinary.cache().is_some());
        assert!(!packets.enables_upstream_client_cache());
        assert!(packets.cache().is_none());
        assert!(owner.enables_upstream_client_cache());
    }
}
