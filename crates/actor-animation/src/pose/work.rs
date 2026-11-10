//! Thread-local work counters keep pose-avoidance assertions inside the owning crate.
use std::cell::Cell;

thread_local! {
    static SAMPLES: Cell<usize> = const { Cell::new(0) };
    static COMPOSITIONS: Cell<usize> = const { Cell::new(0) };
}

/// Records entry into native clip sampling, including calls without active clips.
pub(super) fn sample() {
    SAMPLES.with(|count| count.set(count.get() + 1));
}

/// Records both retained-program and recursive skeleton composition.
pub(super) fn compose() {
    COMPOSITIONS.with(|count| count.set(count.get() + 1));
}

/// Takes the calling test's sampling and composition counts without allocating.
pub(crate) fn take() -> (usize, usize) {
    (
        SAMPLES.with(|count| count.replace(0)),
        COMPOSITIONS.with(|count| count.replace(0)),
    )
}
