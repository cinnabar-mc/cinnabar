#![expect(
    clippy::print_stdout,
    reason = "the tool prints the commands that run the showcase"
)]

//! Builds the world material showcase: both halves of the `materialfx` Experience, its
//! generated art, its server artifact, its signed client part `.cxb`, and, given a client's
//! user data directory, the bundle cache entry that client loads it from.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use material_fx::artifact;

const USAGE: &str = "usage: material-fx --out <dir> [--user-data <client user data dir>]";

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
    if let Some(user_data) = &user_data {
        let (digest, objects) = cinnabar_cxb::seed_cache(&built.cxb, user_data)?;
        println!("seeded {digest} into {}", objects.display());
    }
    println!(
        "bedrock-local-server -material-fx -experiences {} -experience-runtime <experience-runtime> \
         -extension-key {} -extension-audience 127.0.0.1:19132 -extension-cxb {}",
        built.experiences.display(),
        built.server_seed.display(),
        built.cxb_dir.display(),
    );
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
