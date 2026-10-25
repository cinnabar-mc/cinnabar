//! `experience.toml`, an artifact's identity, and `files.toml`, the generated index that every
//! other file must match.
//!
//! The manifest also declares the Experience's client part in its `[client]` table, which the
//! runtime ignores whatever it holds: `cinnabar-cxb build --experience` checks it with the
//! client's own verifier and signs it into the `.cxb`, and the server half of client parts routes
//! by that signed manifest, never by this file. Every other key is the manifest's.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::hex;
use crate::host::Api;
use crate::limits::{MAX_ARTIFACT_ENTRIES, MAX_INDEX_BYTES, MAX_MANIFEST_BYTES, MAX_VERSION_BYTES};

use experience_sdk::artifact::{DATA_SCHEMA, INDEX_FILE, MANIFEST_FILE};

/// The table that declares the client part, which the runtime ignores.
pub const CLIENT_TABLE: &str = "client";
/// The manifest's table that held the index before it moved to [`INDEX_FILE`].
const INLINE_INDEX: &str = "files";
const MAX_ID_BYTES: usize = 32;

/// An artifact's identity, from its manifest, and its index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Matches `^[a-z][a-z0-9_]{0,31}$` and owns the block namespace `<id>:`.
    pub id: String,
    pub version: String,
    /// The server WIT's `major.minor`, one of those the runtime implements.
    pub api: String,
    pub data_schema: u32,
    /// `/`-separated relative path → lowercase hex SHA-256, as [`INDEX_FILE`] lists them.
    pub files: BTreeMap<String, String>,
}

/// The manifest's keys outside `[client]`.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Identity {
    id: String,
    version: String,
    api: String,
    data_schema: u32,
}

/// Reads and verifies the artifact in `dir`: `experience.toml`'s size, the id, version, api and
/// data schema, then `files.toml`, the index. Every file except those two must be indexed under a
/// relative `/`-separated path with its hash, and the artifact may hold nothing but regular files
/// and directories, at most [`MAX_ARTIFACT_ENTRIES`] of them.
pub fn read_manifest(dir: &Path) -> Result<Manifest> {
    let found = artifact_files(dir)?;
    let text = read_text(dir, MANIFEST_FILE, MAX_MANIFEST_BYTES)?;
    let parsing = || format!("parsing {MANIFEST_FILE}");
    let mut document: toml::Table = toml::from_str(&text).with_context(parsing)?;
    document.remove(CLIENT_TABLE);
    ensure!(
        !document.contains_key(INLINE_INDEX),
        "{MANIFEST_FILE} holds [files]: the index is {INDEX_FILE} beside it, which lists the \
         same \"<path>\" = \"<sha256>\" entries without the table header"
    );
    let Identity {
        id,
        version,
        api,
        data_schema,
    } = Identity::deserialize(document).with_context(parsing)?;
    ensure!(
        is_id(&id),
        "invalid id \"{id}\": ids match ^[a-z][a-z0-9_]{{0,31}}$"
    );
    ensure!(
        is_version(&version),
        "invalid version: a version has 1 to {MAX_VERSION_BYTES} bytes and no control characters"
    );
    if Api::of(&api).is_none() {
        let supported: Vec<&str> = Api::ALL.into_iter().map(Api::api).collect();
        bail!("unsupported api \"{api}\"; this runtime implements {supported:?}");
    }
    ensure!(
        data_schema == DATA_SCHEMA,
        "unsupported data-schema {data_schema}; this runtime implements {DATA_SCHEMA}"
    );
    let files = read_index(dir, &found)?;
    Ok(Manifest {
        id,
        version,
        api,
        data_schema,
        files,
    })
}

/// Reads `dir/files.toml` and checks it against `found`, the artifact's files: it names every
/// file but the manifest and itself, under a path inside the artifact, with that file's hash.
fn read_index(dir: &Path, found: &BTreeSet<String>) -> Result<BTreeMap<String, String>> {
    let text = read_text(dir, INDEX_FILE, MAX_INDEX_BYTES)?;
    let files: BTreeMap<String, String> =
        toml::from_str(&text).with_context(|| format!("parsing {INDEX_FILE}"))?;
    for path in files.keys() {
        ensure!(
            is_index_path(path) && path != MANIFEST_FILE && path != INDEX_FILE,
            "invalid path \"{path}\" in {INDEX_FILE}: paths are relative, use '/', stay inside \
             the artifact and exclude {MANIFEST_FILE} and {INDEX_FILE}"
        );
    }
    if let Some(path) = found
        .iter()
        .find(|path| *path != MANIFEST_FILE && *path != INDEX_FILE && !files.contains_key(*path))
    {
        bail!("unindexed file {path}");
    }
    for (path, hash) in &files {
        ensure!(found.contains(path), "indexed file {path} is missing");
        let actual = sha256(&resolve(dir, path)).with_context(|| format!("hashing {path}"))?;
        let actual = hex::encode(&actual);
        ensure!(
            actual == *hash,
            "hash mismatch for {path}: {INDEX_FILE} has {hash}, the file hashes to {actual}"
        );
    }
    Ok(files)
}

/// `dir/name` as UTF-8, refusing more than `limit` bytes before reading them.
fn read_text(dir: &Path, name: &str, limit: usize) -> Result<String> {
    let mut bytes = Vec::new();
    File::open(dir.join(name))
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut bytes))
        .with_context(|| format!("reading {name}"))?;
    ensure!(bytes.len() <= limit, "{name} exceeds {limit} bytes");
    String::from_utf8(bytes).with_context(|| format!("{name} is not UTF-8"))
}

/// Joins a `/`-separated index path onto `dir` with the platform's separator.
pub(crate) fn resolve(dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(dir.to_owned(), |full, part| full.join(part))
}

fn sha256(path: &Path) -> io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().into())
}

/// `^[a-z][a-z0-9_]{0,31}$`.
fn is_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    matches!(bytes.first(), Some(b'a'..=b'z'))
        && bytes.len() <= MAX_ID_BYTES
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// 1 to [`MAX_VERSION_BYTES`] bytes without a control character.
fn is_version(version: &str) -> bool {
    (1..=MAX_VERSION_BYTES).contains(&version.len()) && !version.chars().any(char::is_control)
}

/// A path that can only name something inside the artifact on every platform: `/`-separated
/// components that are never empty, `.` or `..`, with no `\` and no `:` (drive prefixes and
/// alternate data streams).
fn is_index_path(path: &str) -> bool {
    path.split('/')
        .all(|part| !matches!(part, "" | "." | "..") && !part.contains(['\\', ':']))
}

/// Every file below `dir` as a `/`-separated relative path. Symlinks, non-UTF-8 names, anything
/// but regular files and directories, and more than [`MAX_ARTIFACT_ENTRIES`] of those are refused;
/// the walk stops at the first entry over that bound.
fn artifact_files(dir: &Path) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    let mut entries = 0;
    let mut pending = vec![(dir.to_owned(), String::new())];
    while let Some((path, prefix)) = pending.pop() {
        let listing = fs::read_dir(&path).with_context(|| format!("listing {}", path.display()))?;
        for entry in listing {
            entries += 1;
            ensure!(
                entries <= MAX_ARTIFACT_ENTRIES,
                "the artifact holds more than {MAX_ARTIFACT_ENTRIES} files and directories"
            );
            let entry = entry.with_context(|| format!("listing {}", path.display()))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|name| anyhow!("{prefix}{} is not a UTF-8 name", name.display()))?;
            let relative = format!("{prefix}{name}");
            let kind = entry
                .file_type()
                .with_context(|| format!("inspecting {relative}"))?;
            if kind.is_symlink() {
                bail!("{relative} is a symlink");
            } else if kind.is_dir() {
                pending.push((entry.path(), format!("{relative}/")));
            } else if kind.is_file() {
                files.insert(relative);
            } else {
                bail!("{relative} is not a regular file or directory");
            }
        }
    }
    Ok(files)
}
