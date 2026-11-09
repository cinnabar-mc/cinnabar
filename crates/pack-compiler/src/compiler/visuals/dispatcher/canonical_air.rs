//! The registry's one canonical air record, identified from its content.

use super::super::super::*;
use assets::facts::pack::PackBlock as B;

/// Wire identity of the one record that resolves as canonical air for a
/// compiled registry. Derived from registry content at compile time instead
/// of pinning one legacy protocol's sequential ID and network hash, so a
/// protocol-2225 triple whose air sits at different identities compiles the
/// same exact Invisible route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CanonicalAirIdentity {
    pub(super) sequential_id: u32,
    pub(super) network_hash: u32,
}

fn air_candidate(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::Air.identifier()
        && record.canonical_state.as_ref() == "{}"
        && record.flags.contains(BlockFlags::AIR)
        && record.contributor_role == ContributorRole::Air
}

/// Resolves the unique canonical-air identity from registry content.
///
/// Fails closed when no record satisfies the full predicate, when more than
/// one does, or when any other record wearing the `minecraft:air` name fails
/// the predicate (a decoy that would make the canonical identity ambiguous).
/// Air-flagged records under other names stay non-fatal decoys and keep the
/// established diagnostic-stripping route.
pub(super) fn resolve_canonical_air(
    records: &[RegistryRecord],
) -> Result<CanonicalAirIdentity, AssetError> {
    let mut canonical = None;
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == B::Air.identifier())
    {
        if !air_candidate(record) {
            return Err(AssetError::InvalidCompiledAssets {
                detail: format!(
                    "registry record {} names minecraft:air but fails the canonical-air state/flags/role contract",
                    record.sequential_id
                )
                .into(),
            });
        }
        if canonical.replace(record).is_some() {
            return Err(AssetError::InvalidCompiledAssets {
                detail: "registry declares more than one canonical minecraft:air record".into(),
            });
        }
    }
    let canonical = canonical.ok_or_else(|| AssetError::InvalidCompiledAssets {
        detail: "registry declares no canonical minecraft:air record".into(),
    })?;
    Ok(CanonicalAirIdentity {
        sequential_id: canonical.sequential_id,
        network_hash: canonical.network_hash,
    })
}

pub(super) fn is_canonical_air(
    record: &RegistryRecord,
    canonical_air: CanonicalAirIdentity,
) -> bool {
    record.sequential_id == canonical_air.sequential_id
        && record.network_hash == canonical_air.network_hash
        && air_candidate(record)
}
