use super::{PlaySession, WorldIngress};

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Admits the current ordinary item without retiring cached data just because it blocks.
    /// False retains this input while bounded raw read-ahead looks for missing bodies. Recovery
    /// is allowed only when that read-ahead reaches its byte budget or a session boundary.
    pub(super) fn finish_ordinary_ingress(
        &mut self,
        recover: bool,
    ) -> Result<bool, crate::BlobCacheError> {
        let (_, bytes) = self.pending_ordinary.as_ref().expect("ordinary input");
        let resolver = self.blob_cache.as_mut().expect("enabled cache");
        if !resolver.can_accept_ordinary(*bytes)?
            && resolver.ordinary_lane_needs_drain()
            && (!recover || resolver.unblock_ordinary_lane()?)
        {
            return Ok(false);
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
        }?;
        Ok(true)
    }
}
