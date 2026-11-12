//! Resource and behaviour packs a world carries in its own folder, where the dedicated server reads
//! them: pack folders under `resource_packs/` and `behavior_packs/`, listed in highest-priority-first
//! order by `world_resource_packs.json` and `world_behavior_packs.json`.

use std::{
    collections::HashSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use launcher::local_worlds::packs::{PackKind, WorldPack, WorldPacks};
use resource_pack::{MAX_MANIFEST_BYTES, MAX_PACKS, PackSummary, summarize_manifest};
use serde_json::{Value, json};

use super::{
    store::Staging,
    unpack::{UnpackLimits, extract_archive},
    world::{WorldError, failed},
    write_private,
};

const MANIFEST: &str = "manifest.json";

/// The largest pack archive a world accepts. Mojang's limits are unverified, so this and the other
/// [`PACK_LIMITS`] only keep a malicious archive from filling the disk.
pub const MAX_PACK_ARCHIVE_BYTES: u64 = 8 << 30;

/// Bounds a pack archive before anything is written.
pub(super) const PACK_LIMITS: UnpackLimits = UnpackLimits {
    label: "pack file",
    refuse_links: true,
    max_entries: 100_000,
    max_file_bytes: MAX_PACK_ARCHIVE_BYTES,
    max_total_bytes: MAX_PACK_ARCHIVE_BYTES,
    min_ratio_sample: 4096,
    max_entry_ratio: 500.0,
    max_archive_ratio: 100.0,
};

/// Where the server looks for one kind of pack inside a world folder.
struct Files {
    folder: &'static str,
    list: &'static str,
}

fn files(kind: PackKind) -> Files {
    match kind {
        PackKind::Resource => Files {
            folder: "resource_packs",
            list: "world_resource_packs.json",
        },
        PackKind::Behaviour => Files {
            folder: "behavior_packs",
            list: "world_behavior_packs.json",
        },
    }
}

/// A pack the world folder holds a copy of.
struct Installed {
    dir: PathBuf,
    summary: PackSummary,
}

/// The packs of the world whose server folder is `level`.
pub(super) fn read(level: &Path) -> Result<WorldPacks, WorldError> {
    Ok(WorldPacks {
        resource: list(level, PackKind::Resource)?,
        behaviour: list(level, PackKind::Behaviour)?,
    })
}

/// Lists one kind in server order. Entries the world holds no copy of are marked external.
fn list(level: &Path, kind: PackKind) -> Result<Vec<WorldPack>, WorldError> {
    let installed = installed(level, kind);
    Ok(read_entries(level, kind)?
        .iter()
        .filter_map(|entry| {
            let (id, version) = identity(entry)?;
            let held = installed
                .iter()
                .find(|pack| pack.summary.id.to_string() == id && pack.summary.version == version);
            Some(match held {
                Some(pack) => world_pack(&pack.summary),
                None => WorldPack {
                    name: id.clone(),
                    id,
                    version,
                    description: String::new(),
                    external: true,
                },
            })
        })
        .collect())
}

/// The list entry for a pack the world holds a copy of.
fn world_pack(summary: &PackSummary) -> WorldPack {
    WorldPack {
        id: summary.id.to_string(),
        version: summary.version,
        name: summary.name.clone(),
        description: summary.description.clone(),
        external: false,
    }
}

/// Names the packs of `kind` the archive at `source` holds as one list row, installing nothing.
pub(super) fn preview(
    kind: PackKind,
    source: &Path,
    staging: &Staging,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<WorldPack, WorldError> {
    let found = unpack(kind, source, staging, cancelled)?;
    let mut summaries = found.iter().map(|pack| &pack.summary);
    let first = summaries
        .next()
        .ok_or_else(|| WorldError::Rejected(format!("the file holds no {}", kind.noun())))?;
    let mut pack = world_pack(first);
    let more = summaries.count();
    if more > 0 {
        pack.description = format!("This file holds {} {}s", more + 1, kind.noun());
    }
    Ok(pack)
}

/// Adds the packs of `kind` found in the archive at `source` to the front of the world's list,
/// replacing any pack with the same id. A refused file changes nothing.
pub(super) fn add(
    level: &Path,
    kind: PackKind,
    source: &Path,
    staging: &Staging,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(), WorldError> {
    let mut entries = read_entries(level, kind)?;
    let matching = unpack(kind, source, staging, cancelled)?;
    let mut ids = Vec::new();
    for pack in matching {
        if ids.contains(&pack.summary.id) {
            continue;
        }
        ids.push(pack.summary.id);
        let id = pack.summary.id.to_string();
        install(level, kind, &pack)?;
        entries.retain(|entry| !identity(entry).is_some_and(|(held, _)| held == id));
        entries.insert(0, json!({ "pack_id": id, "version": pack.summary.version }));
    }
    write_entries(level, kind, &entries)
}

/// Moves the unpacked `pack` into the world's folder for `kind`, named by its id and replacing any
/// copy of the same id.
fn install(level: &Path, kind: PackKind, pack: &Installed) -> Result<(), WorldError> {
    let folder = level.join(files(kind).folder);
    fs::create_dir_all(&folder).map_err(|error| failed(format!("create pack folder: {error}")))?;
    // A same-id copy under any folder name would load twice.
    for old in installed(level, kind)
        .into_iter()
        .filter(|old| old.summary.id == pack.summary.id)
    {
        remove_dir(&old.dir)?;
    }
    fs::rename(&pack.dir, folder.join(pack.summary.id.to_string()))
        .map_err(|error| failed(format!("install pack: {error}")))
}

/// Copies pack `id` of `kind` from a Bedrock game folder under `roots` into the world, for a list
/// entry the world holds no copy of, and points the entry at the copy's version. The copy matching
/// the entry's version wins, else the newest.
pub(super) fn find(
    level: &Path,
    kind: PackKind,
    id: &str,
    roots: &[PathBuf],
    staging: &Staging,
) -> Result<(), WorldError> {
    let mut entries = read_entries(level, kind)?;
    let (at, wanted) = entries
        .iter()
        .enumerate()
        .find_map(|(at, entry)| {
            let (held, version) = identity(entry)?;
            (held == id).then_some((at, version))
        })
        .ok_or(WorldError::NotFound)?;
    let mut copies = game_packs(roots, kind, id);
    copies.sort_by_key(|pack| (pack.summary.version == wanted, pack.summary.version));
    let Some(found) = copies.pop() else {
        return Err(WorldError::Rejected(format!(
            "no copy of this {} was found in Minecraft's pack folders",
            kind.noun()
        )));
    };
    let copy = Installed {
        dir: staging.unpacked().join("copy"),
        summary: found.summary,
    };
    copy_tree(&found.dir, &copy.dir)?;
    install(level, kind, &copy)?;
    entries[at]["version"] = json!(copy.summary.version);
    write_entries(level, kind, &entries)
}

/// The packs of `kind` with id `id` in the user-installed and development pack folders of the
/// Bedrock game folders `roots`.
fn game_packs(roots: &[PathBuf], kind: PackKind, id: &str) -> Vec<Installed> {
    let folder = files(kind).folder;
    roots
        .iter()
        .flat_map(|root| {
            [
                root.join(format!("development_{folder}")),
                root.join(folder),
            ]
        })
        .flat_map(|folder| pack_dirs(&folder))
        .filter_map(|dir| {
            let summary = read_summary(&dir)?;
            let wanted = match kind {
                PackKind::Resource => summary.resources,
                PackKind::Behaviour => summary.behaviour,
            };
            (wanted && summary.id.to_string() == id).then_some(Installed { dir, summary })
        })
        .collect()
}

/// Copies the pack folder `source` to `destination`, within the archive limits and without
/// following or copying links.
fn copy_tree(source: &Path, destination: &Path) -> Result<(), WorldError> {
    const MAX_DEPTH: usize = 32;
    let too_big = || WorldError::Rejected("the pack folder is too large".to_owned());
    let (mut entries, mut bytes) = (0u64, 0u64);
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf(), 0)];
    while let Some((from, to, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return Err(too_big());
        }
        fs::create_dir_all(&to).map_err(|error| failed(format!("copy pack: {error}")))?;
        let listing = fs::read_dir(&from).map_err(|error| failed(format!("copy pack: {error}")))?;
        for entry in listing {
            let entry = entry.map_err(|error| failed(format!("copy pack: {error}")))?;
            entries += 1;
            if entries > PACK_LIMITS.max_entries {
                return Err(too_big());
            }
            let kind = entry
                .file_type()
                .map_err(|error| failed(format!("copy pack: {error}")))?;
            let (path, target) = (entry.path(), to.join(entry.file_name()));
            if kind.is_dir() {
                pending.push((path, target, depth + 1));
            } else if kind.is_file() {
                bytes += entry
                    .metadata()
                    .map_err(|error| failed(format!("copy pack: {error}")))?
                    .len();
                if bytes > PACK_LIMITS.max_total_bytes {
                    return Err(too_big());
                }
                fs::copy(&path, &target).map_err(|error| failed(format!("copy pack: {error}")))?;
            } else {
                return Err(WorldError::Rejected(
                    "the pack folder contains a link".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

/// Unpacks the archive at `source` into `staging` and returns the packs of `kind` in it, naming
/// the other tab when it holds only the other kind.
fn unpack(
    kind: PackKind,
    source: &Path,
    staging: &Staging,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<Installed>, WorldError> {
    let extension_ok = source
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            resource_pack::PACK_IMPORT_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        });
    if !extension_ok {
        return Err(WorldError::Rejected(format!(
            "choose a {} archive (.mcpack, .mcaddon or .zip)",
            kind.noun()
        )));
    }
    let unpacked = staging.unpacked();
    extract_archive(&unpacked, source, &PACK_LIMITS, cancelled)?;
    let found = find_packs(&unpacked)?;
    let (matching, other): (Vec<_>, Vec<_>) = found.into_iter().partition(|pack| match kind {
        PackKind::Resource => pack.summary.resources,
        PackKind::Behaviour => pack.summary.behaviour,
    });
    if matching.is_empty() {
        let opposite = match kind {
            PackKind::Resource => PackKind::Behaviour,
            PackKind::Behaviour => PackKind::Resource,
        };
        return Err(WorldError::Rejected(if other.is_empty() {
            format!("the file holds no {}", kind.noun())
        } else {
            format!(
                "the file holds a {}; add it on the {}s tab",
                opposite.noun(),
                opposite.noun()
            )
        }));
    }
    Ok(matching)
}

/// Removes pack `id` from the world's list of `kind`, deleting the copy the world holds.
pub(super) fn remove(level: &Path, kind: PackKind, id: &str) -> Result<(), WorldError> {
    let mut entries = read_entries(level, kind)?;
    let before = entries.len();
    entries.retain(|entry| !identity(entry).is_some_and(|(held, _)| held == id));
    if entries.len() == before {
        return Err(WorldError::NotFound);
    }
    write_entries(level, kind, &entries)?;
    for old in installed(level, kind)
        .into_iter()
        .filter(|old| old.summary.id.to_string() == id)
    {
        remove_dir(&old.dir)?;
    }
    Ok(())
}

/// Moves pack `id` to index `to` of the world's list of `kind`.
pub(super) fn move_to(level: &Path, kind: PackKind, id: &str, to: usize) -> Result<(), WorldError> {
    let mut entries = read_entries(level, kind)?;
    let from = entries
        .iter()
        .position(|entry| identity(entry).is_some_and(|(held, _)| held == id))
        .ok_or(WorldError::NotFound)?;
    let entry = entries.remove(from);
    entries.insert(to.min(entries.len()), entry);
    write_entries(level, kind, &entries)
}

/// Reads the list file; a missing file is an empty list and a malformed one is refused, so a
/// hand-edited file is never overwritten.
fn read_entries(level: &Path, kind: PackKind) -> Result<Vec<Value>, WorldError> {
    let path = level.join(files(kind).list);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(failed(format!("read pack list: {error}"))),
    };
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| failed(format!("read pack list: {error}")))?;
    let malformed = || WorldError::Rejected("the world's pack list cannot be read".to_owned());
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(malformed());
    }
    let text = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    match serde_json::from_slice::<Value>(text) {
        Ok(Value::Array(entries)) => Ok(entries),
        _ => Err(malformed()),
    }
}

fn write_entries(level: &Path, kind: PackKind, entries: &[Value]) -> Result<(), WorldError> {
    let path = level.join(files(kind).list);
    let temporary = path.with_extension("json.cinnabar");
    let bytes = serde_json::to_vec_pretty(entries)
        .map_err(|error| failed(format!("write pack list: {error}")))?;
    write_private(&temporary, &bytes)
        .and_then(|()| fs::rename(&temporary, &path))
        .map_err(|error| failed(format!("write pack list: {error}")))
}

/// A list entry's lowercase pack id and version, if it names a pack.
fn identity(entry: &Value) -> Option<(String, [u32; 3])> {
    let id = entry["pack_id"].as_str()?.trim().to_ascii_lowercase();
    let parts = entry["version"].as_array()?;
    let mut version = [0; 3];
    if parts.len() != 3 {
        return None;
    }
    for (slot, part) in version.iter_mut().zip(parts) {
        *slot = u32::try_from(part.as_u64()?).ok()?;
    }
    Some((id, version))
}

/// The packs of `kind` the world folder holds, at most [`MAX_PACKS`] of them.
fn installed(level: &Path, kind: PackKind) -> Vec<Installed> {
    pack_dirs(&level.join(files(kind).folder))
        .into_iter()
        .filter_map(|dir| {
            let summary = read_summary(&dir)?;
            let wanted = match kind {
                PackKind::Resource => summary.resources,
                PackKind::Behaviour => summary.behaviour,
            };
            wanted.then_some(Installed { dir, summary })
        })
        .collect()
}

/// The pack folders directly under `folder`, never following links.
fn pack_dirs(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .take(MAX_PACKS * 4)
        .collect()
}

fn read_summary(dir: &Path) -> Option<PackSummary> {
    let file = fs::File::open(dir.join(MANIFEST)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    summarize_manifest(&bytes).ok()
}

/// The packs unpacked from an archive: its root, or each folder below the root (an add-on bundle).
fn find_packs(root: &Path) -> Result<Vec<Installed>, WorldError> {
    let mut candidates = Vec::new();
    if root.join(MANIFEST).is_file() {
        candidates.push(root.to_path_buf());
    } else {
        candidates.extend(pack_dirs(root));
        // One wrapper folder around a bundle or a single pack.
        if let [wrapper] = candidates.as_slice()
            && !wrapper.join(MANIFEST).is_file()
        {
            let wrapper = wrapper.clone();
            candidates = pack_dirs(&wrapper);
        }
    }
    let mut found: Vec<Installed> = candidates
        .into_iter()
        .filter_map(|dir| {
            let summary = read_summary(&dir)?;
            Some(Installed { dir, summary })
        })
        .collect();
    found.truncate(MAX_PACKS);
    if found.is_empty() {
        return Err(WorldError::Rejected(
            "the file holds no pack manifest".to_owned(),
        ));
    }
    // Extraction cannot create the same folder twice, so one id repeats only across folders.
    let mut seen = HashSet::new();
    found.retain(|pack| seen.insert((pack.summary.id, pack.summary.version)));
    Ok(found)
}

fn remove_dir(dir: &Path) -> Result<(), WorldError> {
    fs::remove_dir_all(dir).map_err(|error| failed(format!("remove pack: {error}")))
}

#[cfg(test)]
#[path = "packs_tests.rs"]
mod tests;
