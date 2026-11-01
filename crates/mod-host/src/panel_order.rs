//! Shared ordering for explicit panel visibility requests.

/// Orders explicit panel opening requests across frame and packet runtimes.
pub(crate) fn next_panel_open_order() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
