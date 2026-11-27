//! Scripted, single-client resident-render benchmarks over developer control.

mod files;
mod observe;
mod paired;
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
    let index = args.iter().position(|arg| {
        arg == "--benchmark" || arg == "--benchmark-compare" || arg == "--benchmark-paired"
    })?;
    Some((|| {
        let operand = |offset: usize| {
            args.get(index + offset)
                .map(Path::new)
                .ok_or_else(|| format!("missing operand {offset} after {:?}", args[index]))
        };
        let (first, second) = (operand(1)?, operand(2)?);
        if args[index] == "--benchmark-paired" {
            let rounds = operand(3)?
                .to_str()
                .and_then(|rounds| rounds.parse().ok())
                .ok_or("paired rounds must be a whole number")?;
            paired::run(repo, first, second, rounds, operand(4)?)
        } else if args[index] == "--benchmark-compare" {
            report::compare(first, second)
        } else {
            run::run(repo, first, second)
        }
    })())
}
