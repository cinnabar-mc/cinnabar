//! Canonical package and runtime authority for the portable mod-library worker.

use crate::modding::registration::{removal, selection};
use launcher::mods::{Action, InstalledMod, Permission, Status};
use launcher_host::mod_library::port::{Backend, Retirement, Selection};
use std::path::{Path, PathBuf};

pub(super) struct AppBackend;

impl Selection for selection::Registered {
    /// Returns the runtime's retained source identity.
    fn component(&self) -> &Path {
        &self.entry.component
    }
    /// Reads enabled state directly from the canonical entry.
    fn enabled(&self) -> bool {
        self.entry.enabled
    }
    /// Preserves request- and reload-matched runtime status.
    fn status(&self) -> Status {
        self.status.clone()
    }
}

impl Retirement for removal::Permit {
    /// Checks that the runtime has not advanced beyond the retired generation.
    fn confirm(&self) -> Result<(), String> {
        self.confirm()
    }
    /// Protects settings paths still retained by live runtime instances.
    fn shared_companion(&self, path: &Path) -> bool {
        self.shared_companion(path)
    }
}

impl Backend for AppBackend {
    type Registered = selection::Registered;
    type Declarations = (Option<mod_host::ModGrants>, ui::mod_metadata::ModMetadata);
    type Receipt = selection::Receipt;
    type Permit = removal::Permit;
    const MOD_API_REVISIONS_JSON: &'static str = mod_api::COMPATIBILITY_REVISIONS_JSON;
    const HOST_REVISION: &'static str = diagnostics::session::COMMIT;
    const MAX_SOURCE_BYTES: usize = mod_host::packet_mod::MAX_PACKET_MOD_BYTES;

    /// Uses the same schema and matched statuses as the registration watcher.
    fn registered(directory: &Path) -> Vec<Self::Registered> {
        selection::registered(directory)
    }
    /// Keeps declarations opaque to the service and uses the canonical parser.
    fn declarations(path: &Path) -> Self::Declarations {
        match mod_host::declarations(path) {
            Ok(declared) => (declared.requested.ok().flatten(), declared.metadata),
            Err(_) => (None, Default::default()),
        }
    }
    /// Builds permissions from the grant declaration without maintaining another capability list.
    fn describe(
        item: &mut InstalledMod,
        declared: &Self::Declarations,
        current: Option<&Self::Registered>,
    ) {
        let (requested, metadata) = declared;
        let grants = current.map(|current| &current.entry.grants);
        item.metadata = metadata.clone();
        item.permissions = mod_host::ModGrants::CAPABILITIES
            .iter()
            .map(|capability| Permission {
                name: capability.label.into(),
                description: capability.description.trim().into(),
                enabled: grants.is_some_and(capability.enabled),
                requested: requested.as_ref().map(capability.enabled),
                requested_allowed: requested.as_ref().is_some_and(|requested| {
                    grants.is_some_and(|grants| (capability.requested_allowed)(grants, requested))
                }),
                sensitive: capability.sensitive,
            })
            .collect();
    }
    /// Retains regular-file, link and package-size admission checks.
    fn read_source(path: &Path) -> Result<Vec<u8>, String> {
        mod_host::read_source(path).map_err(|error| error.to_string())
    }
    /// Retains validation for every supported frame and packet package format.
    fn validate_source(bytes: &[u8]) -> Result<(), String> {
        mod_host::validate_source(bytes).map_err(|error| format!("{error:#}"))
    }
    /// Parses exact download bytes and rejects invalid new permission declarations.
    fn update_metadata(
        bytes: &[u8],
        check_requested: bool,
    ) -> Result<ui::mod_metadata::ModMetadata, String> {
        let declared = mod_host::declarations_snapshot(bytes).map_err(|error| error.to_string())?;
        if check_requested {
            declared
                .requested
                .map_err(|error| format!("invalid requested permissions: {error}"))?;
        }
        Ok(declared.metadata)
    }
    /// Edits the canonical registration while the service owns command ordering and identities.
    fn apply(
        action: Action,
        directory: &Path,
        component: Option<&Path>,
        request: String,
    ) -> Result<(), String> {
        if action == Action::ReloadAll {
            return selection::reload_all(directory, request);
        }
        let component = component.ok_or("That mod is no longer installed.")?;
        match action {
            Action::Enable(_) => selection::select(directory, component, true, request),
            Action::Disable(_) => selection::disable(directory, component, request),
            Action::Reload(_) => selection::reload(directory, component, request),
            Action::MoveUp(_) | Action::MoveDown(_) => selection::move_entry(
                directory,
                component,
                matches!(action, Action::MoveUp(_)),
                request,
            ),
            Action::Permission(_, capability, enabled) => {
                selection::permission(directory, component, capability, enabled, request)
            }
            Action::AllowRequested(_) => selection::allow_requested(directory, component, request),
            Action::GrantAll(_) => selection::grant_all(directory, component, request),
            Action::RevokeAll(_) => selection::revoke_all(directory, component, request),
            _ => Err("Unsupported mod selection action.".into()),
        }
    }
    /// Withdraws retained authority before requesting runtime retirement.
    fn forget(
        directory: &Path,
        component: &Path,
        request: String,
    ) -> Result<Self::Receipt, String> {
        selection::forget(directory, component, request)
    }
    /// Rechecks the exact selection receipt before each deletion.
    fn confirm(receipt: &Self::Receipt, directory: &Path) -> Result<(), String> {
        receipt.confirm(directory)
    }
    /// Uses the same companion identity as the runtime settings writer.
    fn settings_companion(component: &Path) -> PathBuf {
        mod_host::settings_companion(component)
    }
}
