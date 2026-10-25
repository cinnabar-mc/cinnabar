use super::{PlaySession, WorldEvent, WorldIngress};

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Admits retained input, or recovers its cached blockers at a raw byte or session boundary.
    /// True restarts ready delivery; false retains the input while body read-ahead waits.
    pub(super) fn finish_ordinary_ingress(
        &mut self,
        recover: bool,
    ) -> Result<bool, crate::BlobCacheError> {
        let (input, bytes) = self.pending_ordinary.as_ref().expect("ordinary input");
        let boundary = matches!(input, WorldIngress::Event(WorldEvent::ChangeDimension(_)));
        let resolver = self.blob_cache.as_mut().expect("enabled cache");
        if (!resolver.can_accept_ordinary(*bytes)? || boundary)
            && resolver.ordinary_lane_needs_drain()
        {
            if !(recover || boundary) {
                return Ok(false);
            }
            if resolver.unblock_ordinary_lane()? {
                return Ok(true);
            }
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
