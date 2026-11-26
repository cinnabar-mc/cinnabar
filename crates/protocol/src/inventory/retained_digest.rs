//! Digests of retained item bytes, remembered for the allocations hashed most recently.
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Retained byte allocations whose digests this thread computed most recently.
const RETAINED_DIGESTS: usize = 4;

/// The SHA-256 of `bytes`, reusing a digest this thread computed for the same allocation. Each
/// remembered digest holds its allocation, so the address cannot be reused while it is
/// remembered, and the bytes behind an `Arc<[u8]>` cannot change: a reused digest is always the
/// digest of these bytes. Per-frame checks of the selected stack then hash it once.
pub(super) fn retained_digest(bytes: &Arc<[u8]>) -> [u8; 32] {
    type Remembered = ([Option<(Arc<[u8]>, [u8; 32])>; RETAINED_DIGESTS], usize);
    thread_local! {
        static REMEMBERED: std::cell::RefCell<Remembered> =
            const { std::cell::RefCell::new(([const { None }; RETAINED_DIGESTS], 0)) };
    }
    REMEMBERED.with_borrow_mut(|(entries, next)| {
        if let Some((_, digest)) = entries
            .iter()
            .flatten()
            .find(|(held, _)| Arc::ptr_eq(held, bytes))
        {
            return *digest;
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        entries[*next] = Some((Arc::clone(bytes), digest));
        *next = (*next + 1) % RETAINED_DIGESTS;
        digest
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A remembered digest is the digest of the same bytes, including after other allocations
    /// with equal or different contents have been hashed and the remembered slots wrapped.
    #[test]
    fn remembered_digests_match_fresh_ones() {
        let fresh = |bytes: &[u8]| -> [u8; 32] { Sha256::digest(bytes).into() };
        let first: Arc<[u8]> = Arc::from(&[1_u8, 2, 3][..]);
        assert_eq!(retained_digest(&first), fresh(&first));
        assert_eq!(retained_digest(&first), fresh(&first));
        let others: Vec<Arc<[u8]>> = (0..=RETAINED_DIGESTS as u8)
            .map(|byte| Arc::from(&[byte; 5][..]))
            .collect();
        for bytes in &others {
            assert_eq!(retained_digest(bytes), fresh(bytes));
        }
        let equal: Arc<[u8]> = Arc::from(&first[..]);
        assert_eq!(retained_digest(&equal), fresh(&first));
        assert_eq!(retained_digest(&first), fresh(&first));
        let empty: Arc<[u8]> = Arc::from(&[][..]);
        assert_eq!(retained_digest(&empty), fresh(&[]));
    }
}
