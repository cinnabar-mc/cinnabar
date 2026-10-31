//! Runtime authority used by the portable library, without importing a guest engine or schema.

use launcher::mods::{Action, InstalledMod, Status};
use std::path::{Path, PathBuf};
use ui::mod_metadata::ModMetadata;

/// Read-only access to the runtime's canonical registration entry and matching status.
pub trait Selection {
    /// Returns the component identity retained by this registration.
    fn component(&self) -> &Path;
    /// Reports whether the user enabled this entry.
    fn enabled(&self) -> bool;
    /// Returns the status for this entry's current request and reload counter.
    fn status(&self) -> Status;
}

/// Authority held after guest detachment and completion of its final settings writes.
pub trait Retirement {
    /// Rejects a permit superseded by a later runtime generation.
    fn confirm(&self) -> Result<(), String>;
    /// Protects a companion still owned by a surviving settings writer.
    fn shared_companion(&self, path: &Path) -> bool;
}

/// Connects library operations to canonical package inspection and registration authority.
/// Associated data stays opaque so grants, receipts and persisted schemas have one owner.
pub trait Backend: 'static {
    type Registered: Selection;
    type Declarations;
    type Receipt;
    type Permit: Retirement;

    /// The canonical package byte limit used by both import picking and update downloading.
    const MAX_SOURCE_BYTES: usize;
    /// JSON array of API revisions supported by this native build.
    const MOD_API_REVISIONS_JSON: &'static str = "[]";
    /// Full commit stamped into this native binary; unstamped builds cannot request updates.
    const HOST_REVISION: &'static str = "";

    /// Reads the canonical registration with statuses matched to its request and reload counters.
    fn registered(directory: &Path) -> Vec<Self::Registered>;
    /// Inspects a source once for the library's revision-keyed declaration cache.
    fn declarations(path: &Path) -> Self::Declarations;
    /// Populates a card from cached declarations and the entry's current grants.
    fn describe(
        item: &mut InstalledMod,
        declared: &Self::Declarations,
        current: Option<&Self::Registered>,
    );
    /// Reads a regular, bounded source using the package owner's file admission rules.
    fn read_source(path: &Path) -> Result<Vec<u8>, String>;
    /// Validates the exact package bytes without admitting or granting a running instance.
    fn validate_source(bytes: &[u8]) -> Result<(), String>;
    /// Parses update metadata, optionally requiring a valid requested-permissions declaration.
    fn update_metadata(bytes: &[u8], check_requested: bool) -> Result<ModMetadata, String>;
    /// Persists a selection or permission action through the canonical registration editor.
    fn apply(
        action: Action,
        directory: &Path,
        component: Option<&Path>,
        request: String,
    ) -> Result<(), String>;
    /// Removes retained registration authority and returns a receipt of the exact published bytes.
    fn forget(directory: &Path, component: &Path, request: String)
    -> Result<Self::Receipt, String>;
    /// Rejects any changed registration immediately before each file deletion.
    fn confirm(receipt: &Self::Receipt, directory: &Path) -> Result<(), String>;
    /// Returns the package owner's canonical persisted-settings companion path.
    fn settings_companion(component: &Path) -> PathBuf;
}
