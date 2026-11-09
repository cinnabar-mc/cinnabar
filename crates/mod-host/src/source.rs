//! Source admission shared by mod imports, metadata reads and instance preparation.

use crate::{MAX_COMPONENT_SOURCE_BYTES, ModGrants, ModHost, local_files, packet_mod};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Instant};

/// The binary forms accepted by the library; core reactors run on the packet host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Component,
    Packet,
}

impl SourceKind {
    /// Classifies core reactors without executing their code; validation checks their ABI later.
    pub fn of(bytes: &[u8]) -> Self {
        if bytes.starts_with(b"\0asm\x01\0\0\0") || bytes.starts_with(b"(module") {
            Self::Packet
        } else {
            Self::Component
        }
    }

    /// The existing source ceiling for this host, independent of filename or claimed grants.
    pub fn byte_limit(self) -> usize {
        match self {
            Self::Component => MAX_COMPONENT_SOURCE_BYTES,
            Self::Packet => packet_mod::MAX_PACKET_MOD_BYTES,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComponentKind {
    Frame,
    Packet,
    Invalid,
}

/// Retains bounded admission facts to avoid recompiling unchanged registrations.
fn component_kind(bytes: &[u8]) -> ComponentKind {
    use std::{
        collections::BTreeMap,
        sync::{Mutex, PoisonError},
    };
    static CACHE: Mutex<BTreeMap<[u8; 32], ComponentKind>> = Mutex::new(BTreeMap::new());
    let identity: [u8; 32] = Sha256::digest(bytes).into();
    if let Some(kind) = CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&identity)
    {
        return *kind;
    }
    let kind = if validate_packet_source(bytes).is_ok() {
        ComponentKind::Packet
    } else if ModHost::validate_component(bytes).is_ok() {
        ComponentKind::Frame
    } else {
        ComponentKind::Invalid
    };
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() >= mod_api::MAX_LOADED_MODS {
        cache.pop_first();
    }
    cache.insert(identity, kind);
    kind
}

/// Detects packet runtimes independently of persisted permissions, including wrapped reactors.
pub fn is_packet_source(bytes: &[u8]) -> bool {
    SourceKind::of(bytes) == SourceKind::Packet || component_kind(bytes) == ComponentKind::Packet
}

/// Recognizes a valid frame component without executing its initialization or trusting grants.
pub fn is_frame_source(bytes: &[u8]) -> bool {
    SourceKind::of(bytes) == SourceKind::Component && component_kind(bytes) == ComponentKind::Frame
}

/// Reads one regular source, retaining the smaller component limit for ordinary mods.
pub fn read_source(path: &Path) -> Result<Vec<u8>> {
    let bytes = local_files::read(path, packet_mod::MAX_PACKET_MOD_BYTES)?;
    check_size(&bytes)?;
    Ok(bytes)
}

impl local_files::ObservationCache {
    /// Observes a mod source with the same classification and byte ceilings as imports.
    pub fn source(&mut self, path: &Path, now: Instant) -> Result<local_files::Snapshot> {
        let snapshot = self.read(path, packet_mod::MAX_PACKET_MOD_BYTES, now)?;
        check_size(&snapshot.bytes)?;
        Ok(snapshot)
    }
}

/// Validates a source without running it; larger reactors use the packet host's ABI and cache.
pub fn validate_source(bytes: &[u8]) -> Result<()> {
    check_size(bytes)?;
    match SourceKind::of(bytes) {
        SourceKind::Component => ModHost::validate_component(bytes).or_else(|component_error| {
            validate_packet_source(bytes).map_err(|packet_error| {
                component_error.context(format!("packet mod validation: {packet_error:#}"))
            })
        }),
        SourceKind::Packet => validate_packet_source(bytes),
    }
}

/// Checks the packet world's ABI without executing guest code or granting a live instance.
fn validate_packet_source(bytes: &[u8]) -> Result<()> {
    packet_mod::CompiledPacketMod::compile(
        bytes,
        &ModGrants {
            packets: true,
            wasi: true,
            ..Default::default()
        },
    )
    .map(|_| ())
}

/// Applies the owning host's source ceiling even when bytes came from a captured snapshot.
pub(crate) fn check_size(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= SourceKind::of(bytes).byte_limit(),
        "source exceeds byte limit"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_frame_fixture_loads_with_stale_packet_authority() {
        let Some(path) = std::env::var_os("CINNABAR_FRAME_MOD_FIXTURE") else {
            eprintln!(
                "missing fixture: CINNABAR_FRAME_MOD_FIXTURE; skipping released frame admission"
            );
            return;
        };
        let path = Path::new(&path);
        let bytes = read_source(path).unwrap();
        let mut grants = crate::declarations_snapshot(&bytes)
            .unwrap()
            .requested
            .unwrap()
            .unwrap_or_default();
        grants.packets = true;
        grants.wasi = true;
        let old_error = packet_mod::CompiledPacketMod::compile(&bytes, &grants)
            .err()
            .expect("old grant-based routing rejects this frame component");
        assert!(format!("{old_error:#}").contains("link packet mod"));
        assert!(!is_packet_source(&bytes));
        grants.packets = false;
        grants.wasi = false;
        let host = ModHost::load_with_grants(path, grants).unwrap();
        assert!(host.is_active());
    }

    #[test]
    fn large_reactors_keep_their_own_limit_and_components_keep_the_smaller_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mod.wasm");
        let mut bytes = vec![0; MAX_COMPONENT_SOURCE_BYTES + 1];
        bytes[..8].copy_from_slice(b"\0asm\x01\0\0\0");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_source(&path).unwrap(), bytes);
        let mut observations = local_files::ObservationCache::default();
        let now = Instant::now();
        assert_eq!(
            observations.source(&path, now).unwrap().bytes.as_ref(),
            bytes
        );
        bytes[..8].copy_from_slice(b"\0asm\x0d\0\x01\0");
        std::fs::write(&path, &bytes).unwrap();
        assert!(read_source(&path).is_err());
        assert!(
            observations
                .source(&path, now + local_files::CONTENT_REVALIDATION_INTERVAL)
                .is_err()
        );
        bytes[..8].copy_from_slice(b"\0asm\x01\0\0\0");
        assert!(
            validate_source(&bytes).is_err(),
            "size admission must not admit invalid reactors"
        );
    }
}
