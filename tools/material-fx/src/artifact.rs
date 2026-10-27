//! The showcase's server artifact and client part bundle, built from the guest modules and the
//! generated art.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, ensure};
use experience_sdk::artifact::{ASSETS_DIR, INDEX_FILE, MANIFEST_FILE, SERVER_WASM};
use sha2::{Digest, Sha256};

use crate::{art, diorama, pictures};

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
    /// Whether the build deleted a fixture world installed with a block it no longer has.
    pub world_reset: bool,
}

/// The client part's generated textures, by bundle path, as `experience.toml` lists them.
pub fn client_textures() -> Vec<(String, Vec<u8>)> {
    let picture = |image: image::RgbaImage| art::png(&image);
    vec![
        ("textures/noise.png".into(), art::noise()),
        ("textures/stars.png".into(), art::stars()),
        (
            "textures/diorama.png".into(),
            picture(diorama::image(&diorama::grid())),
        ),
        (
            "textures/painting.png".into(),
            picture(pictures::painting()),
        ),
        ("textures/rune.png".into(), picture(pictures::rune())),
        ("textures/skybox.png".into(), picture(pictures::skybox())),
        (
            "textures/lenticular.png".into(),
            picture(pictures::lenticular()),
        ),
    ]
}

/// Each showcase block's Bedrock look while its material is revoked: base and accent colours.
/// The server half registers exactly these blocks, which `tests/showcase.rs` checks.
pub const BLOCK_LOOKS: [(&str, [u8; 3], [u8; 3]); 14] = [
    ("bottle", [200, 225, 240], [150, 180, 200]),
    ("aquarium", [40, 90, 160], [110, 110, 115]),
    ("mineshaft", [40, 34, 30], [110, 110, 115]),
    ("arena", [40, 42, 50], [90, 95, 110]),
    ("blackhole", [10, 8, 16], [60, 40, 90]),
    ("canvas", [120, 90, 50], [210, 165, 60]),
    ("glintore", [120, 120, 125], [90, 220, 215]),
    ("pulseore", [120, 120, 125], [200, 20, 10]),
    ("wallstone", [120, 120, 125], [95, 95, 100]),
    ("snowglobe", [220, 235, 250], [110, 70, 40]),
    ("lavalamp", [90, 30, 120], [240, 140, 40]),
    ("screen", [20, 40, 25], [60, 62, 70]),
    ("rune", [160, 155, 145], [90, 140, 220]),
    ("vortex", [30, 12, 50], [90, 50, 140]),
];

/// The server half's block textures, by artifact path.
fn block_textures() -> Vec<(String, Vec<u8>)> {
    BLOCK_LOOKS
        .iter()
        .enumerate()
        .map(|(seed, (name, base, accent))| {
            (
                format!("{ASSETS_DIR}/{name}.png"),
                art::png(&pictures::block_look(*base, *accent, seed as u32)),
            )
        })
        .collect()
}

/// Writes `bytes` to `path`, creating its directory.
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// The artifact's `files.toml` index: each of `files` by its SHA-256, one `"<path>" = "<sha256>"`
/// line each.
pub fn index(files: &[(&str, &[u8])]) -> String {
    let mut index = String::new();
    for (path, bytes) in files {
        let digest = Sha256::digest(bytes);
        let hex = digest.iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        let _ = writeln!(index, "\"{path}\" = \"{hex}\"");
    }
    index
}

/// The directories under `out` that [`build`] writes afresh: the artifacts and the bundles.
fn generated_dirs(out: &Path) -> [PathBuf; 2] {
    [out.join("experiences"), out.join("cxb")]
}

/// Removes what an earlier [`build`] generated under `out`, so no file a previous showcase
/// had (an artifact file its manifest no longer indexes, an old bundle) survives a rebuild.
/// Keys and the world stay.
fn clear_generated(out: &Path) -> Result<()> {
    for dir in generated_dirs(out) {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("clearing {}", dir.display()));
            }
        }
    }
    Ok(())
}

/// Every block id the server half registers, as the Experience runtime names them.
pub fn block_ids() -> Vec<String> {
    BLOCK_LOOKS
        .iter()
        .map(|(name, ..)| format!("{ID}:{name}"))
        .collect()
}

/// Builds the artifact under `out/experiences/materialfx`, keys under `out/keys`, and the
/// client part bundle under `out/cxb`, after clearing what an earlier build generated there.
/// Existing keys are kept, so a client that pinned the server keeps trusting it. The fixture
/// world is deleted when it was installed with a block this build no longer has
/// ([`crate::world::reset_if_blocks_dropped`]).
pub fn build(root: &Path, out: &Path, guests: &Guests) -> Result<Built> {
    let source = std::fs::read(root.join(SOURCE).join(MANIFEST_FILE))?;
    clear_generated(out)?;
    let world_reset = crate::world::reset_if_blocks_dropped(out, ID, &block_ids())?;
    let artifact = out.join("experiences").join(ID);
    let textures = client_textures();
    let blocks = block_textures();
    let mut files: Vec<(&str, &[u8])> = vec![(SERVER_WASM, &guests.server)];
    files.extend(
        blocks
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    files.extend(
        textures
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    for (path, bytes) in &files {
        write(&artifact.join(path), bytes)?;
    }
    let manifest_path = artifact.join(MANIFEST_FILE);
    write(&manifest_path, &source)?;
    write(&artifact.join(INDEX_FILE), index(&files).as_bytes())?;
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
    let [experiences, cxb_dir] = generated_dirs(out);
    let cxb = cxb_dir.join(format!("{ID}.cxb"));
    write(&cxb, &bundle.bytes)?;
    Ok(Built {
        experiences,
        cxb_dir,
        cxb,
        server_seed: server,
        world_reset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_removes_stale_artifact_files_and_keeps_keys_and_the_world() {
        let out = tempfile::tempdir().unwrap();
        let stale = out
            .path()
            .join("experiences")
            .join(ID)
            .join("assets/energy.png");
        write(&stale, b"old").unwrap();
        write(&out.path().join("cxb/old.cxb"), b"old").unwrap();
        write(&out.path().join("keys/server.seed"), b"key").unwrap();
        write(&crate::world::dir(out.path()).join("level.dat"), b"world").unwrap();
        clear_generated(out.path()).unwrap();
        assert!(!stale.exists());
        assert!(!out.path().join("cxb").exists());
        assert!(out.path().join("keys/server.seed").exists());
        assert!(crate::world::dir(out.path()).join("level.dat").exists());
        // A first build has nothing to clear.
        clear_generated(&out.path().join("fresh")).unwrap();
    }
}
