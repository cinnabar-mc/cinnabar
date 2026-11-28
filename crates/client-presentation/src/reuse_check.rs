//! Allocation-free checks that a reused value matches a fresh rebuild.

use std::{
    fmt::{self, Debug, Write},
    hash::{DefaultHasher, Hasher},
};

/// Whether `reused` and `fresh` print the same `Debug` output. Both are streamed through a
/// hasher instead of formatted into strings, so debug and test builds can run the check on
/// every frame without allocating.
pub(crate) fn same_debug(reused: &impl Debug, fresh: &impl Debug) -> bool {
    debug_digest(reused) == debug_digest(fresh)
}

/// Hashes `value`'s `Debug` output as it is written.
fn debug_digest(value: &impl Debug) -> u64 {
    struct Sink(DefaultHasher);
    impl Write for Sink {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.0.write(text.as_bytes());
            Ok(())
        }
    }
    let mut sink = Sink(DefaultHasher::new());
    write!(sink, "{value:?}").expect("hashing never fails");
    sink.0.finish()
}

#[cfg(test)]
mod tests {
    use super::same_debug;

    #[test]
    fn compares_debug_output_without_allocating() {
        let reused = (1.5f32, Some("bow"), [0u8; 4]);
        let before = test_alloc::thread_allocations();
        let same = same_debug(&reused, &(1.5f32, Some("bow"), [0u8; 4]));
        let differs = same_debug(&reused, &(-1.5f32, Some("bow"), [0u8; 4]));
        let allocated = test_alloc::thread_allocations() - before;
        assert!(same);
        assert!(!differs);
        assert_eq!(allocated, 0, "the check must not allocate");
    }
}
