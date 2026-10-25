use super::{PlaySession, WorldIngress};

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Admits the current ordinary item without retiring cached data just because it blocks.
    /// At the existing lane quota, retain this one input across receives while recovery and
    /// older events drain. No later transport input is read until this item is admitted.
    pub(super) fn finish_ordinary_ingress(&mut self) -> Result<(), crate::BlobCacheError> {
        let (_, bytes) = self.pending_ordinary.as_ref().expect("ordinary input");
        let resolver = self.blob_cache.as_mut().expect("enabled cache");
        if !resolver.can_accept_ordinary(*bytes)? && resolver.unblock_ordinary_lane()? {
            return Ok(());
        }
        let (input, bytes) = self
            .pending_ordinary
            .take()
            .expect("retained ordinary input");
        match input {
            WorldIngress::Event(event) => resolver.accept_world_event(event, bytes),
            WorldIngress::LevelChunk(event, payload) => {
                resolver.accept_level_chunk_bytes(event, payload, bytes)
            }
        }
    }
}
