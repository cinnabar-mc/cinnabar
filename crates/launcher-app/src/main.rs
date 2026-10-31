//! `cinnabar-launcher`: opens the launcher window, or with `--snapshot` renders one of its
//! screens to a PNG without a window or GPU.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use launcher_app::snapshot::{self, State};

/// Parses `--snapshot <state> --out <png> [--width <px>] [--height <px>]`, else opens the window.
#[expect(
    clippy::print_stderr,
    reason = "the launcher installs no log subscriber; stderr is its only diagnostic channel"
)]
fn main() {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        std::process::exit(launcher_app::run());
    };
    if let Err(message) = snapshot_command(&first, args) {
        eprintln!("{message}");
        eprintln!(
            "usage: cinnabar-launcher [--snapshot <{}> --out <png> [--width <px>] [--height <px>]]",
            State::NAMES.join("|")
        );
        std::process::exit(2);
    }
}

/// Runs the offline snapshot command line, or explains what is wrong with it.
fn snapshot_command(first: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    if first != "--snapshot" {
        return Err(format!("unknown argument {first}"));
    }
    let name = args.next().ok_or("--snapshot requires a state")?;
    let state = State::parse(&name).ok_or_else(|| format!("unknown state {name}"))?;
    let (mut out, mut size) = (None, [1280u32, 720u32]);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} requires a value"));
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(value()?)),
            "--width" => size[0] = value()?.parse().map_err(|error| format!("{error}"))?,
            "--height" => size[1] = value()?.parse().map_err(|error| format!("{error}"))?,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    let out = out.ok_or("--snapshot requires --out <png>")?;
    snapshot::write(state, size, &out).map_err(|error| format!("{error:#}"))
}
