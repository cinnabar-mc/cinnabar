//! The local server's fixture world for the showcase, and when a rebuild must start it afresh.
//!
//! A world records every block each installed Experience ever registered
//! (`experience-data/_installed.json`), and the server refuses to open it once one of them is
//! gone, because the world may still hold that block. That rule protects real worlds, so the
//! tool keeps it: when a rebuild drops a block the fixture world was installed with, the tool
//! deletes the fixture world, which holds nothing but the gallery, and the server builds a new
//! one. Adding blocks keeps the world.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The world directory under the tool's output, which the printed server command passes as
/// `-dir`.
pub fn dir(out: &Path) -> PathBuf {
    out.join("world")
}

/// The installation record of the world in `world`, as the local server writes it.
fn installed(world: &Path) -> PathBuf {
    world.join("experience-data").join("_installed.json")
}

/// The blocks `world`'s installation record lists for Experience `id`; `None` when the world
/// records no installation of it.
fn installed_blocks(world: &Path, id: &str) -> Result<Option<Vec<String>>> {
    let path = installed(world);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let record: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    Ok(record["blocks"][id].as_array().map(|blocks| {
        blocks
            .iter()
            .filter_map(|block| block.as_str().map(str::to_owned))
            .collect()
    }))
}

/// Deletes the fixture world under `out` when it was installed with a block of Experience `id`
/// that `blocks` no longer has, and returns whether it did. An unreadable record also resets
/// the world, since the server could not open it either.
pub fn reset_if_blocks_dropped(out: &Path, id: &str, blocks: &[String]) -> Result<bool> {
    let world = dir(out);
    let dropped = match installed_blocks(&world, id) {
        Ok(Some(installed)) => installed.iter().any(|block| !blocks.contains(block)),
        Ok(None) => false,
        Err(_) => true,
    };
    if dropped {
        std::fs::remove_dir_all(&world)
            .with_context(|| format!("removing the stale world {}", world.display()))?;
    }
    Ok(dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world under a new output directory installed with `blocks` of Experience `x`.
    fn world_with(blocks: &[&str]) -> tempfile::TempDir {
        let out = tempfile::tempdir().unwrap();
        let record = installed(&dir(out.path()));
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        let record_json = serde_json::json!({"schema": 1, "ids": ["x"], "blocks": {"x": blocks}});
        std::fs::write(&record, record_json.to_string()).unwrap();
        std::fs::write(dir(out.path()).join("level.dat"), b"world").unwrap();
        out
    }

    fn names(blocks: &[&str]) -> Vec<String> {
        blocks.iter().map(|block| (*block).to_owned()).collect()
    }

    #[test]
    fn a_dropped_block_resets_the_world_and_added_blocks_keep_it() {
        let out = world_with(&["x:a", "x:b"]);
        assert!(!reset_if_blocks_dropped(out.path(), "x", &names(&["x:a", "x:b", "x:c"])).unwrap());
        assert!(dir(out.path()).join("level.dat").exists());
        assert!(reset_if_blocks_dropped(out.path(), "x", &names(&["x:a"])).unwrap());
        assert!(!dir(out.path()).exists());
        // With no world at all there is nothing to reset.
        assert!(!reset_if_blocks_dropped(out.path(), "x", &names(&["x:a"])).unwrap());
    }

    #[test]
    fn another_experiences_blocks_never_reset_the_world() {
        let out = world_with(&["x:a"]);
        assert!(!reset_if_blocks_dropped(out.path(), "y", &[]).unwrap());
        assert!(dir(out.path()).exists());
    }
}
