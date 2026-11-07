//! Marketplace declarations must match the exact signed request before any guest is instantiated.

use launcher_host::marketplace::{Guard, Manifest, managed};
use mod_host::ModGrants;
use std::path::Path;

/// Parses a release's signed grant set, rejecting capabilities this client does not know.
fn signed_grants(manifest: &Manifest) -> Result<ModGrants, String> {
    serde_json::from_value(manifest.grants.clone())
        .map_err(|error| format!("Unsupported signed mod permissions: {error}"))
}

/// Compares requested capabilities and an optional declared version against signed release
/// metadata, returning the signed grants on success.
pub(super) fn declarations(bytes: &[u8], manifest: &Manifest) -> Result<ModGrants, String> {
    let declarations = mod_host::declarations_snapshot(bytes).map_err(|error| error.to_string())?;
    let actual = declarations
        .requested
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let signed = signed_grants(manifest)?;
    if actual != signed {
        return Err("The component's permission request differs from its signed release.".into());
    }
    if declarations
        .metadata
        .version
        .as_ref()
        .is_some_and(|version| version != &manifest.version)
    {
        return Err("The component's version differs from its signed release.".into());
    }
    Ok(signed)
}

/// Caps a component's retained grants to the signed release currently authorized for its path.
///
/// Local components keep their grants. A managed component without current, supported signed
/// evidence keeps none; its admission is then refused by [`Guard::check`] or [`declarations`].
pub(super) fn cap(component: &Path, grants: &mut ModGrants, guard: &Guard, now: u64) {
    if !managed(component) {
        return;
    }
    let signed = guard
        .manifest(component, now)
        .and_then(|manifest| signed_grants(&manifest).ok())
        .unwrap_or_default();
    grants.cap(&signed);
}

/// Builds original binary declarations without executing a component.
#[cfg(test)]
pub(super) fn component_fixture(grants: &str) -> Vec<u8> {
    let name = b"cinnabar:grants";
    let mut bytes = b"\0asm\x0d\0\x01\0".to_vec();
    bytes.extend([0, (1 + name.len() + grants.len()) as u8, name.len() as u8]);
    bytes.extend(name);
    bytes.extend(grants.as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_command_values_and_unknown_permissions_are_checked_before_admission() {
        let mut manifest = Manifest {
            slug: "fixture-mod".into(),
            version: "1.0.0".into(),
            sha256: "a".repeat(64),
            bytes: 1,
            grants: serde_json::json!({"commands":["help"]}),
            approved_at: 1,
            client_revision: "a".repeat(40),
            policy: "fixture".into(),
            publication: "manual".into(),
        };
        assert!(declarations(&component_fixture(r#"{"commands":["help"]}"#), &manifest).is_ok());
        assert!(declarations(&component_fixture(r#"{"commands":["op"]}"#), &manifest).is_err());
        manifest.grants = serde_json::json!({"future_capability":true});
        assert!(declarations(&component_fixture("{}"), &manifest).is_err());
    }
}
