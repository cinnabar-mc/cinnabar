use std::collections::{HashSet, VecDeque};

use super::MAX_CLIENT_BLOB_CACHE_ENTRIES;

/// Bounded session history of requested hashes, independent of terrain transaction ownership.
/// It retains no bodies or pins. Responses can warm the normal cache after ordinary work retires
/// a transaction, but only a new cached packet can publish that terrain. Oldest distinct hashes
/// expire at the existing cache-entry ceiling; live dependencies remain eligible separately.
#[derive(Debug, Default)]
pub(super) struct RequestedBlobHashes {
    hashes: HashSet<u64>,
    order: VecDeque<u64>,
}

impl RequestedBlobHashes {
    /// Remembers each distinct missing hash without retaining unbounded request metadata.
    pub(super) fn record(&mut self, hashes: &[u64]) {
        for &hash in hashes {
            if self.hashes.contains(&hash) {
                continue;
            }
            if self.hashes.len() == MAX_CLIENT_BLOB_CACHE_ENTRIES {
                let oldest = self.order.pop_front().expect("full request history");
                self.hashes.remove(&oldest);
            }
            self.hashes.insert(hash);
            self.order.push_back(hash);
        }
    }

    /// Whether this session recently requested the hash, even if its terrain was retired.
    pub(super) fn contains(&self, hash: u64) -> bool {
        self.hashes.contains(&hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob_cache::tests::cached_level_chunk;
    use crate::{
        BlobCacheReady, BlobCacheResolver, BlockUpdateEvent, ClientBlobCache, Packet, WorldEvent,
        client_blob_hash,
    };
    use bedrock_protocol_1_26_60::{
        packets::ClientCacheMissResponse, types::MissingBlobData, wire,
    };

    /// Builds a response with explicit claimed hashes for validation and recovery tests.
    fn miss_response(blobs: Vec<(u64, Vec<u8>)>) -> ClientCacheMissResponse {
        ClientCacheMissResponse {
            missing_blobs: blobs
                .into_iter()
                .map(|(hash, payload)| MissingBlobData {
                    blob_id: wire::U64LE(hash),
                    blob_data: payload.into(),
                })
                .collect(),
        }
    }

    /// Retires a missing column through the ordinary lane and drains its recovery and update.
    fn retire_column(resolver: &mut BlobCacheResolver, payload: &[u8]) -> u64 {
        let hash = client_blob_hash(payload);
        resolver
            .accept_cached_packet(Packet::new(cached_level_chunk(1, vec![hash])))
            .unwrap();
        resolver
            .accept_world_event(
                WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                    dimension: 0,
                    position: [16, 0, 0],
                    layer: 0,
                    network_id: 1,
                }]),
                0,
            )
            .unwrap();
        assert!(resolver.unblock_ordinary_lane().unwrap());
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
        ));
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::WorldEvent(WorldEvent::BlockUpdates(_)))
        ));
        assert!(resolver.pop_ready().is_none());
        hash
    }

    /// Only a fresh cached packet may publish a body delivered after terrain retirement.
    #[test]
    fn requested_blob_after_retirement_only_warms_cache() {
        let payload = b"retired terrain";
        let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
        let hash = retire_column(&mut resolver, payload);
        assert_eq!(resolver.stats().cache_pinned_bytes, 0);
        assert!(
            resolver
                .accept_miss_response(miss_response(vec![(hash, payload.to_vec())]))
                .unwrap()
        );
        assert!(resolver.cache().contains(hash));
        assert!(resolver.pop_ready().is_none());
        let status = resolver
            .accept_cached_packet(Packet::new(cached_level_chunk(1, vec![hash])))
            .unwrap();
        assert_eq!(status.have(), [hash]);
        assert!(status.missing().is_empty());
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::Packet(_))
        ));
    }

    /// Remembering a request must not suppress the recovery's missing-ID status.
    #[test]
    fn requested_blob_history_does_not_suppress_recovery_misses() {
        let payload = b"rerequested terrain";
        let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
        let hash = retire_column(&mut resolver, payload);
        let status = resolver
            .accept_cached_packet(Packet::new(cached_level_chunk(1, vec![hash])))
            .unwrap();
        assert_eq!(status.missing(), [hash]);
        resolver
            .accept_miss_response(miss_response(vec![(hash, payload.to_vec())]))
            .unwrap();
        assert!(matches!(
            resolver.pop_ready(),
            Some(BlobCacheReady::Packet(_))
        ));
    }

    /// Every reset invalidates history even when ordinary retirement left no live transaction.
    #[test]
    fn requested_blob_history_is_cleared_at_reset_boundaries() {
        for reset in 0..3 {
            let payload = b"old requested terrain";
            let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
            let hash = retire_column(&mut resolver, payload);
            match reset {
                0 => resolver.reset_pending(),
                1 => resolver.recover_pending().unwrap(),
                _ => {
                    resolver.arm_fast_transfer_reset();
                    assert!(
                        resolver
                            .reset_pending_for_fast_transfer_candidate()
                            .unwrap()
                    );
                }
            }
            assert!(
                !resolver
                    .accept_miss_response(miss_response(vec![(hash, payload.to_vec())]))
                    .unwrap()
            );
            assert!(!resolver.cache().contains(hash));
            assert!(resolver.pop_ready().is_none());
        }
    }

    /// Retired requested bodies still require whole-response integrity and duplicate checks.
    #[test]
    fn requested_blob_history_preserves_atomic_integrity_validation() {
        for conflicting_duplicate in [false, true] {
            let retired = b"retired requested terrain";
            let wanted = b"current requested terrain";
            let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
            let hash = retire_column(&mut resolver, retired);
            let wanted_hash = client_blob_hash(wanted);
            resolver
                .accept_cached_packet(Packet::new(cached_level_chunk(2, vec![wanted_hash])))
                .unwrap();
            let mut blobs = vec![(wanted_hash, wanted.to_vec())];
            if conflicting_duplicate {
                blobs.push((hash, retired.to_vec()));
            }
            blobs.push((hash, b"wrong body".to_vec()));
            assert!(!resolver.accept_miss_response(miss_response(blobs)).unwrap());
            assert!(!resolver.cache().contains(hash));
            assert!(!resolver.cache().contains(wanted_hash));
            assert_eq!(resolver.stats().miss_response_integrity_rejection, 1);
        }
    }

    /// Repeated requests do not consume history slots or let its two indexes diverge.
    #[test]
    fn requested_blob_history_is_bounded_and_deduplicated() {
        let mut history = RequestedBlobHashes::default();
        for hash in 0..MAX_CLIENT_BLOB_CACHE_ENTRIES as u64 {
            history.record(&[hash, hash]);
        }
        let newest = MAX_CLIENT_BLOB_CACHE_ENTRIES as u64;
        history.record(&[newest, newest]);
        assert!(!history.contains(0));
        assert!(history.contains(1));
        assert!(history.contains(newest));
        assert_eq!(history.hashes.len(), MAX_CLIENT_BLOB_CACHE_ENTRIES);
        assert_eq!(history.order.len(), history.hashes.len());
        assert_eq!(
            history.order.iter().copied().collect::<HashSet<_>>(),
            history.hashes
        );
    }
}
