//! Stable fixture copying and input identities; links are rejected rather than followed.

use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

/// Decodes a local JSON input with its path in any error.
pub(super) fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_reader(std::io::BufReader::new(file))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Hashes file contents without loading a world database or client binary into memory.
pub(super) fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Copies a frozen tree and hashes sorted path/content pairs from the copied bytes.
pub(super) fn copy_tree(source: &Path, target: &Path) -> Result<String, String> {
    std::fs::create_dir(target).map_err(|e| e.to_string())?;
    let mut entries = std::fs::read_dir(source)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut hash = Sha256::new();
    for entry in entries {
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = target.join(entry.file_name());
        let digest = if kind.is_dir() {
            hash.update(b"directory");
            copy_tree(&entry.path(), &path)?
        } else if kind.is_file() {
            hash.update(b"file");
            std::fs::copy(entry.path(), &path).map_err(|e| e.to_string())?;
            hash_file(&path)?
        } else {
            return Err("fixture templates must contain only regular files and directories".into());
        };
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "fixture path is not UTF-8")?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(digest.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Writes one new JSON artifact without replacing an earlier run.
pub(super) fn write(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    use std::io::Write;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}
