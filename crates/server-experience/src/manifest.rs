//! Signed offer scope and immutable bundle index.

// BEGIN GENERATED EXTENSION CONTRACT
// Generated from crates/server-experience/schema/extension.json; do not edit.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub permissions: BTreeSet<Permission>,
    pub origins: BTreeSet<String>,
    pub memory_bytes: u64,
    pub gpu_bytes: u64,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub personal_mods: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackageOffer {
    pub id: String,
    pub publisher_key: String,
    pub digest: String,
    pub bytes: u64,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub version: u16,
    pub audience: String,
    pub server_key: String,
    pub revision: u64,
    pub expires_unix: u64,
    pub scope: Scope,
    pub packages: Vec<PackageOffer>,
    pub fallback: String,
    pub carrier: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u16,
    pub api: u16,
    pub id: String,
    pub publisher_key: String,
    pub package_version: String,
    pub permissions: BTreeSet<Permission>,
    pub component: Option<String>,
    pub channels: Vec<crate::wire::Channel>,
    pub actions: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub templates: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub files: Vec<ContentFile>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Ui,
    ModalUi,
    Input,
    Messaging,
    Scene,
    Media,
    Items,
    WorldDraw,
    EntityObserve,
    EntityHighlight,
    RenderThroughWalls,
    RenderMaterial,
    RenderWorldMaterial,
    RenderPost,
    RenderWorldMaterialOverride,
}

/// Restricts identifiers to the shared portable namespace.
pub fn identifier(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_IDENTIFIER_BYTES
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b":_-.".contains(&b))
}
/// Lists permissions with application adapters in the developer preview.
pub fn implemented_permissions() -> BTreeSet<Permission> {
    BTreeSet::from([
        Permission::Ui,
        Permission::ModalUi,
        Permission::Input,
        Permission::Messaging,
        Permission::WorldDraw,
        Permission::EntityObserve,
        Permission::EntityHighlight,
        Permission::RenderThroughWalls,
        Permission::RenderMaterial,
        Permission::RenderPost,
    ])
}
// END GENERATED EXTENSION CONTRACT

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{crypto::fixed_hex, policy::*};

/// Whether `origin` (an ASCII-serialized origin such as `https://cxb.invalid:443`) names a
/// reserved `.invalid` host (RFC 6761), which never resolves. Local servers name one as the
/// origin of bundles a client must already hold in its cache, so the client never contacts it.
pub fn reserved_origin(origin: &str) -> bool {
    let host = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    let host = host.split(['/', '?', '#']).next().unwrap_or_default();
    if host.starts_with('[') {
        return false;
    }
    let host = host.rsplit_once(':').map_or(host, |(name, _)| name);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "invalid" || host.ends_with(".invalid")
}

impl Scope {
    /// Validates the user-visible scope against host ceilings.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.memory_bytes <= MAX_SESSION_MEMORY,
            "memory grant too large"
        );
        ensure!(self.gpu_bytes <= MAX_GPU_BYTES, "GPU grant too large");
        ensure!(self.origins.len() <= MAX_ORIGINS, "too many origins");
        ensure!(
            self.personal_mods.len() <= MAX_BUNDLES,
            "too many personal mod grants"
        );
        for digest in &self.personal_mods {
            fixed_hex::<32>(digest)?;
        }
        for origin in &self.origins {
            let url = crate::fetch::approved_url(origin, &self.origins)?;
            ensure!(
                url.origin().ascii_serialization() == *origin,
                "origin must be canonical"
            );
        }
        Ok(())
    }
}

impl Offer {
    /// Checks all pre-consent metadata without contacting an external origin.
    pub fn validate(&self, audience: &str, now_unix: u64) -> Result<()> {
        ensure!(
            self.version == WIRE_VERSION,
            "unsupported extension version"
        );
        ensure!(self.audience == audience, "wrong server audience");
        fixed_hex::<32>(&self.server_key)?;
        ensure!(self.expires_unix > now_unix, "offer expired");
        ensure!(
            self.expires_unix - now_unix <= MAX_OFFER_LIFETIME_SECS,
            "offer lifetime too long"
        );
        ensure!(
            self.carrier == protocol::EXPERIENCE_CHANNEL,
            "unsupported carrier"
        );
        ensure!(
            !self.packages.is_empty() && self.packages.len() <= MAX_BUNDLES,
            "invalid package count"
        );
        ensure!(
            plain_text(&self.fallback, MAX_FALLBACK_BYTES),
            "invalid fallback description"
        );
        self.scope.validate()?;
        let mut ids = BTreeSet::new();
        let mut total = 0u64;
        for package in &self.packages {
            ensure!(
                identifier(&package.id) && ids.insert(&package.id),
                "invalid package identity"
            );
            fixed_hex::<32>(&package.publisher_key)?;
            fixed_hex::<32>(&package.digest)?;
            ensure!(
                package.bytes > 0 && package.bytes <= MAX_BUNDLE_BYTES as u64,
                "invalid bundle size"
            );
            total = total
                .checked_add(package.bytes)
                .ok_or_else(|| anyhow::anyhow!("size overflow"))?;
            crate::fetch::approved_url(&package.url, &self.scope.origins)?;
        }
        ensure!(
            total <= MAX_EXPANDED_BYTES,
            "aggregate bundle size exceeded"
        );
        Ok(())
    }
}

impl Manifest {
    /// Bounds the modal screen files: every template is an indexed `ui/<name>.json`, and every
    /// image under `textures/` is a bounded PNG.
    pub fn validate_screens(&self) -> Result<()> {
        ensure!(self.templates.len() <= MAX_TEMPLATES, "too many templates");
        for template in &self.templates {
            ensure!(
                crate::screen::template_root(template).is_some(),
                "invalid template path"
            );
            ensure!(
                self.files
                    .iter()
                    .any(|file| &file.path == template && file.bytes <= MAX_TEMPLATE_BYTES as u64),
                "template missing from index or too large"
            );
        }
        let mut textures = 0;
        for file in self
            .files
            .iter()
            .filter(|file| file.path.starts_with(TEXTURE_DIR))
        {
            let image = file.path.ends_with(".png") && file.bytes <= MAX_TEXTURE_BYTES as u64;
            let sidecar = file.path.ends_with(".json") && file.bytes <= MAX_TEMPLATE_BYTES as u64;
            ensure!(image || sidecar, "invalid texture file");
            textures += 1;
        }
        ensure!(textures <= MAX_TEXTURES, "too many textures");
        Ok(())
    }

    /// A signed `namespace` must be well formed and unreserved, and a bundle asking for
    /// `render_world_material` must sign one, since its bindings may name nothing else.
    pub fn validate_namespace(&self) -> Result<()> {
        ensure!(
            self.namespace.as_deref().is_none_or(world_namespace),
            "invalid namespace"
        );
        ensure!(
            self.namespace.is_some()
                || !self.permissions.contains(&Permission::RenderWorldMaterial),
            "render_world_material needs a namespace"
        );
        ensure!(
            self.permissions.contains(&Permission::RenderWorldMaterial)
                || !self
                    .permissions
                    .contains(&Permission::RenderWorldMaterialOverride),
            "render_world_material_override needs render_world_material"
        );
        Ok(())
    }
}

/// Where a bundle keeps the images its screens may draw.
pub const TEXTURE_DIR: &str = "textures/";

/// The JSON-UI namespace every template of bundle `id` declares: the id with each character a
/// JSON-UI name cannot hold replaced by `_`.
pub fn template_namespace(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Keeps remote strings out of control markup and trusted UI formatting.
pub fn plain_text(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.len() <= limit && !text.chars().any(|c| c.is_control() || c == '§')
}

/// Lets the developer client advertise `render_world_material_override` when set to `1`; it is
/// denied otherwise.
pub const WORLD_MATERIAL_OVERRIDE_ENV: &str = "CINNABAR_DEV_WORLD_MATERIAL_OVERRIDE";

/// What the developer client advertises: the above plus media playback onto scene quads,
/// reading the session's items and programmable world materials, and restyling identifiers
/// outside a bundle's namespace only when [`WORLD_MATERIAL_OVERRIDE_ENV`] opts in.
pub fn developer_permissions() -> BTreeSet<Permission> {
    developer_permissions_with(
        std::env::var_os(WORLD_MATERIAL_OVERRIDE_ENV).is_some_and(|value| value == "1"),
    )
}

/// [`developer_permissions`], with the world material override opted in or not.
pub fn developer_permissions_with(world_material_override: bool) -> BTreeSet<Permission> {
    let mut permissions = implemented_permissions();
    permissions.extend([
        Permission::Scene,
        Permission::Media,
        Permission::Items,
        Permission::RenderWorldMaterial,
    ]);
    if world_material_override {
        permissions.insert(Permission::RenderWorldMaterialOverride);
    }
    permissions
}

/// Namespaces no bundle may claim as its own; binding their identifiers needs
/// `render_world_material_override`.
pub const RESERVED_NAMESPACES: [&str; 1] = ["minecraft"];

/// Longest signed `namespace`, in bytes.
pub const MAX_NAMESPACE_BYTES: usize = 64;

/// Whether `namespace` is one a manifest may sign: lowercase ASCII letters, digits, `_`, `-`
/// and `.`, with no `:`, at most [`MAX_NAMESPACE_BYTES`], and not reserved.
pub fn world_namespace(namespace: &str) -> bool {
    !namespace.is_empty()
        && namespace.len() <= MAX_NAMESPACE_BYTES
        && namespace
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-.".contains(&b))
        && !RESERVED_NAMESPACES.contains(&namespace)
}

/// Whether `identifier` (`namespace:name`) lies in `namespace`; the name must be non-empty.
pub fn in_namespace(identifier: &str, namespace: &str) -> bool {
    identifier
        .strip_prefix(namespace)
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|name| !name.is_empty() && !name.contains(':'))
        && crate::manifest::identifier(identifier)
}

#[cfg(test)]
mod reserved_origin_tests {
    use super::reserved_origin;

    #[test]
    fn only_reserved_invalid_hosts_are_reserved_origins() {
        for origin in [
            "https://cxb.invalid",
            "https://CXB.Invalid:443",
            "https://invalid",
        ] {
            assert!(reserved_origin(origin), "{origin}");
        }
        for origin in [
            "https://invalid.example",
            "https://media.example:8443",
            "https://[::1]:443",
            "https://127.0.0.1:19133",
        ] {
            assert!(!reserved_origin(origin), "{origin}");
        }
    }
}
