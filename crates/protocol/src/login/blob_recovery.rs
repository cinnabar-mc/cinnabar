use std::future::{Future, poll_fn};
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::FutureExt;

use super::PlaySession;

/// Cinnabar's recovery interval for unresolved cached SubChunk replies. Expiry retries terrain rather
/// than rejecting late bodies; this is an internal liveness policy.
const BLOB_RECOVERY_INTERVAL: Duration = Duration::from_secs(5);

/// One host timer covers the replayable replies present when it starts. Newer transactions
/// receive their own interval after this group resolves or recovers.
pub(super) struct BlobRecovery {
    through: u64,
    timer: Mutex<bridge::tasks::Timer>,
    expired: bool,
}

impl BlobRecovery {
    /// Polls a host deadline once to completion, retaining expiry across cancelled receives.
    fn poll_expired(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.expired {
            self.expired = self
                .timer
                .get_mut()
                .expect("exclusive recovery timer")
                .as_mut()
                .poll(cx)
                .is_ready();
        }
        if self.expired {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    /// Waits for work without postponing a retained recovery deadline. Expiry leaves work
    /// owned by its caller so normal cancellation-safe intake can resume after recovery.
    pub(super) async fn until<T>(
        recovery: &mut Option<Self>,
        work: impl Future<Output = T>,
    ) -> Option<T> {
        let Some(recovery) = recovery else {
            return Some(work.await);
        };
        tokio::select! {
            biased;
            () = poll_fn(|cx| recovery.poll_expired(cx)) => None,
            result = work => Some(result),
        }
    }
}

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Services replayable cached replies even during continuous input or ready output. A single
    /// sequence boundary reuses the resolver's pending order instead of another deadline queue.
    pub(super) fn recover_stalled_blobs(&mut self) -> Result<(), crate::BlobCacheError> {
        let resolver = self.blob_cache.as_mut().expect("enabled cache");
        let Some((first, last)) = resolver.pending_recovery_range() else {
            self.blob_recovery = None;
            return Ok(());
        };
        if self
            .blob_recovery
            .as_ref()
            .is_some_and(|recovery| recovery.through < first)
        {
            self.blob_recovery = None;
        }
        let recovery = self.blob_recovery.get_or_insert_with(|| BlobRecovery {
            through: last,
            timer: Mutex::new(self.transport.sleep(BLOB_RECOVERY_INTERVAL)),
            expired: false,
        });
        if poll_fn(|cx| recovery.poll_expired(cx))
            .now_or_never()
            .is_some()
        {
            let recovery = self.blob_recovery.take().expect("expired recovery");
            resolver.recover_pending_through(recovery.through)?;
        }
        Ok(())
    }
}
