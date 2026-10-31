//! A stdio MCP server that launches a `developer-control` build of the client and drives it
//! over its loopback endpoint: joins, input, chat, cinematic cameras, state, waits,
//! screenshots and fixed-clock recordings.
// The tool list is one `json!` value; its nesting exceeds the default macro recursion limit.
#![recursion_limit = "256"]

mod benchmark;
mod commands;
mod definitions;
mod processes;
#[cfg(test)]
mod tests;
mod tools;

use std::path::PathBuf;

#[expect(
    clippy::print_stderr,
    reason = "the benchmark CLI reports failures on stderr without changing the stdio MCP protocol"
)]
fn main() {
    let repo = repo_argument()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    if let Some(result) = benchmark::cli(&repo) {
        if let Err(error) = result {
            eprintln!("benchmark: {error}");
            std::process::exit(1);
        }
        return;
    }
    mcp_stdio::serve(&mut tools::Server::new(repo));
}

/// `--repo <checkout>`: where binaries, `.local` and relative paths resolve; default cwd.
fn repo_argument() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--repo" {
            return args.next().map(PathBuf::from);
        }
        if let Some(path) = arg.strip_prefix("--repo=") {
            return Some(PathBuf::from(path));
        }
    }
    None
}
