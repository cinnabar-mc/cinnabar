//! Scripted, single-client resident-render benchmarks over developer control.

mod files;
mod observe;
mod plan;
mod report;
mod run;
#[cfg(test)]
mod tests;
mod trace;

use plan::{Plan, Route};
use std::path::Path;

/// Handles explicit benchmark CLI modes, leaving ordinary stdio MCP invocation unchanged.
pub(crate) fn cli(repo: &Path) -> Option<Result<(), String>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let index = args
        .iter()
        .position(|arg| arg == "--benchmark" || arg == "--benchmark-compare")?;
    Some((|| {
        let first = Path::new(
            args.get(index + 1)
                .ok_or("expected two paths after benchmark option")?,
        );
        let second = Path::new(
            args.get(index + 2)
                .ok_or("expected two paths after benchmark option")?,
        );
        if args[index] == "--benchmark-compare" {
            report::compare(first, second)
        } else {
            run::run(repo, first, second)
        }
    })())
}
