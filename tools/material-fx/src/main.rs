#![expect(
    clippy::print_stdout,
    reason = "the tool prints the commands that run the showcase"
)]

//! Builds the world material showcase: both halves of the `materialfx` Experience, its
//! generated art, its server artifact, its signed client part `.cxb`, and, given a client's
//! user data directory, the bundle cache entry that client loads it from.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use material_fx::{artifact, command};

const USAGE: &str = "usage: material-fx --out <dir> [--user-data <client user data dir>]";
/// A checkout-built client keeps its user data in the workspace's `.local`, so the bundle is
/// seeded there unless `--user-data` names another client's directory.
const CHECKOUT_USER_DATA: &str = ".local";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut out, mut user_data) = (None, None);
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args.next().context(USAGE)?;
        match flag.as_str() {
            "--out" => out = Some(PathBuf::from(value)),
            "--user-data" => user_data = Some(PathBuf::from(value)),
            _ => bail!(USAGE),
        }
    }
    let out = out.context(USAGE)?;
    let root = workspace_root();
    let built = artifact::build(&root, &out, &artifact::Guests::build(&root)?)?;
    let user_data = user_data.unwrap_or_else(|| root.join(CHECKOUT_USER_DATA));
    let (digest, objects) = cinnabar_cxb::seed_cache(&built.cxb, &user_data)?;
    println!("seeded {digest} into {}", objects.display());
    println!("{}", command::server_command(&root, &out, &built));
    Ok(())
}

/// The workspace root this tool was built in.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("tools/material-fx sits two levels below the workspace root")
        .to_path_buf()
}
