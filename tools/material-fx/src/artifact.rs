//! The showcase's server artifact and client part bundle, built from the guest modules and the
//! generated art.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

use crate::art;

/// The showcase Experience's id, which is also its client part's bundle id and namespace.
pub const ID: &str = "materialfx";
/// Where the showcase's sources live, relative to the workspace root.
const SOURCE: &str = "examples/experiences/material-fx";

/// The two halves' core modules.
pub struct Guests {
    pub server: Vec<u8>,
    pub client: Vec<u8>,
}

impl Guests {
    /// Builds both halves for `wasm32-unknown-unknown` in release.
    pub fn build(root: &Path) -> Result<Self> {
        let target = root.join("target/experience-guests");
        // One invocation per guest: built together, Cargo unifies the SDK's features, and the
        // client part would then carry the server world's imports, which no client helper
        // provides.
        for package in ["experience-material-fx", "experience-material-fx-client"] {
            let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .current_dir(root)
                .args(["build", "--release", "--locked"])
                .args(["--target", "wasm32-unknown-unknown", "-p", package])
                .arg("--target-dir")
                .arg(&target)
                .status()
                .context("running cargo for the showcase guests")?;
            ensure!(
                status.success(),
                "building the showcase guest {package} failed"
            );
        }
        let module = |name: &str| {
            let path = target.join("wasm32-unknown-unknown/release").join(name);
            std::fs::read(&path).with_context(|| format!("reading {}", path.display()))
        };
        Ok(Self {
            server: module("experience_material_fx.wasm")?,
            client: module("experience_material_fx_client.wasm")?,
        })
    }
}

/// What [`build`] wrote.
pub struct Built {
    /// The directory `-experiences` takes, holding the artifact directory.
    pub experiences: PathBuf,
    /// The directory `-extension-cxb` takes, holding the client part bundle.
    pub cxb_dir: PathBuf,
    pub cxb: PathBuf,
    /// The server key `-extension-key` takes.
    pub server_seed: PathBuf,
}

/// The client part's generated textures, by bundle path, as `experience.toml` lists them.
pub fn client_textures() -> [(&'static str, Vec<u8>); 2] {
    [
        ("textures/noise.png", art::noise()),
        ("textures/stars.png", art::stars()),
    ]
}

/// The server half's block textures, by artifact path.
fn block_textures() -> [(&'static str, Vec<u8>); 2] {
    [
        ("assets/energy.png", art::energy_block()),
        ("assets/portal.png", art::portal_block()),
    ]
}

/// Writes `bytes` to `path`, creating its directory.
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// `experience.toml` with every other artifact file indexed under `[files]` by its SHA-256.
pub fn manifest(source: &str, files: &[(&str, &[u8])]) -> String {
    let mut manifest = source.trim_end().to_owned();
    manifest.push('\n');
    for (path, bytes) in files {
        let digest = Sha256::digest(bytes);
        let hex = digest.iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        let _ = writeln!(manifest, "\"{path}\" = \"{hex}\"");
    }
    manifest
}

/// Builds the artifact under `out/experiences/materialfx`, keys under `out/keys`, and the
/// client part bundle under `out/cxb`. Existing keys are kept, so a client that pinned the
/// server keeps trusting it.
pub fn build(root: &Path, out: &Path, guests: &Guests) -> Result<Built> {
    let source = std::fs::read_to_string(root.join(SOURCE).join("experience.toml"))?;
    let artifact = out.join("experiences").join(ID);
    let textures = client_textures();
    let blocks = block_textures();
    let mut files: Vec<(&str, &[u8])> = vec![("server.wasm", &guests.server)];
    files.extend(blocks.iter().map(|(path, bytes)| (*path, bytes.as_slice())));
    files.extend(
        textures
            .iter()
            .map(|(path, bytes)| (*path, bytes.as_slice())),
    );
    for (path, bytes) in &files {
        write(&artifact.join(path), bytes)?;
    }
    let manifest_path = artifact.join("experience.toml");
    write(&manifest_path, manifest(&source, &files).as_bytes())?;
    let keys = out.join("keys");
    std::fs::create_dir_all(&keys)?;
    let (publisher, server) = (keys.join("publisher.seed"), keys.join("server.seed"));
    for seed in [&publisher, &server] {
        if !seed.exists() {
            cinnabar_cxb::keys::generate(seed)?;
        }
    }
    let bundle = cinnabar_cxb::bundle::build(
        cinnabar_cxb::bundle::Source::read(&manifest_path)?,
        &guests.client,
        &[],
        &cinnabar_cxb::keys::read(&publisher)?,
    )?;
    let cxb_dir = out.join("cxb");
    let cxb = cxb_dir.join(format!("{ID}.cxb"));
    write(&cxb, &bundle.bytes)?;
    Ok(Built {
        experiences: out.join("experiences"),
        cxb_dir,
        cxb,
        server_seed: server,
    })
}
