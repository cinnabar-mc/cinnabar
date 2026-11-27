//! Counted regions for deterministic instruction budgets.
//!
//! `devtool instruction-budgets` runs every test named with [`TEST_PREFIX`] under Callgrind and
//! compares the instructions retired inside its [`count`] call with a committed budget. Callgrind
//! collects only while the calling thread is inside the region, so work handed to other threads
//! is not counted. Outside Callgrind, [`count`] just runs the work.

use std::cell::Cell;

/// Test-name prefix (the last path segment) that makes a test an instruction budget.
pub const TEST_PREFIX: &str = "instruction_budget_";

macro_rules! counted_region {
    ($symbol:ident) => {
        /// Unmangled symbol Callgrind toggles collection on.
        pub const REGION_SYMBOL: &str = stringify!($symbol);

        #[unsafe(no_mangle)]
        #[inline(never)]
        fn $symbol(work: &mut dyn FnMut()) {
            work();
        }

        /// Runs `work` inside the counted region.
        fn enter(work: &mut dyn FnMut()) {
            $symbol(work);
        }
    };
}

counted_region!(cinnabar_counted_instructions);

thread_local! {
    static COUNTED: Cell<bool> = const { Cell::new(false) };
}

/// Runs `work` once inside the counted region and returns its result.
///
/// Warm caches and settle state before calling it, so the budget covers steady work only.
///
/// # Panics
///
/// Panics unless it is called at most once from a test thread whose test name starts with
/// [`TEST_PREFIX`], so every counted region is discovered and has exactly one budget.
pub fn count<T>(work: impl FnOnce() -> T) -> T {
    let thread = std::thread::current();
    let test = thread
        .name()
        .and_then(|name| name.rsplit("::").next())
        .unwrap_or_default();
    assert!(
        test.starts_with(TEST_PREFIX),
        "test_instructions::count runs only in tests named `{TEST_PREFIX}*`, not {:?}",
        thread.name()
    );
    assert!(
        !COUNTED.replace(true),
        "test_instructions::count runs once per test"
    );
    let mut work = Some(work);
    let mut output = None;
    enter(&mut || output = work.take().map(|work| work()));
    output.expect("the counted region ran its work")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `body` on a thread named like a budget test, so these checks are not budgets.
    fn on_budget_thread<T: Send + 'static>(
        body: impl FnOnce() -> T + Send + 'static,
    ) -> std::thread::Result<T> {
        std::thread::Builder::new()
            .name(format!("tests::{TEST_PREFIX}probe"))
            .spawn(body)
            .expect("spawn the probe thread")
            .join()
    }

    #[test]
    fn count_returns_the_work_result() {
        assert_eq!(on_budget_thread(|| count(|| 6 * 7)).unwrap(), 42);
    }

    #[test]
    #[should_panic(expected = "runs only in tests named")]
    fn count_rejects_tests_without_the_budget_prefix() {
        count(|| ());
    }

    #[test]
    fn count_rejects_a_second_region() {
        on_budget_thread(|| count(|| ())).expect("one region per test");
        assert!(
            on_budget_thread(|| {
                count(|| ());
                count(|| ());
            })
            .is_err()
        );
    }
}
