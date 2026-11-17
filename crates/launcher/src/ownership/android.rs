//! Android's local-install proof: Mojang's Minecraft package, signed by Mojang and installed by a
//! store that sells it. Best-effort: `PackageManager` metadata only, never the game's files.

/// [`PACKAGE`] as a literal, for links built at compile time.
macro_rules! package {
    () => {
        "com.mojang.minecraftpe"
    };
}

/// The full game; the trial package never proves ownership.
pub const PACKAGE: &str = package!();

/// SHA-256 of the certificate Mojang signs [`PACKAGE`] with (CN=Mojang AB, issued 2011).
pub const SIGNER_SHA256: [u8; 32] = [
    0x31, 0xbe, 0x40, 0x09, 0x6f, 0x93, 0x1c, 0xd7, 0xf1, 0x1d, 0x5e, 0x26, 0x2d, 0x2b, 0x2c, 0x43,
    0x7c, 0x44, 0x38, 0x5f, 0xb4, 0xec, 0xbc, 0x10, 0x13, 0xd9, 0x5a, 0x74, 0x35, 0x81, 0x6f, 0x9c,
];

/// Installers of record that sell Minecraft: Google Play and the Amazon Appstore.
pub const STORES: [&str; 2] = ["com.android.vending", "com.amazon.venezia"];

/// The Play listing, then its web page for devices without a store app.
pub const STORE_LISTING: [&str; 2] = [
    concat!("market://details?id=", package!()),
    concat!("https://play.google.com/store/apps/details?id=", package!()),
];

/// Installs started by `adb` carry the shell as their initiator, whatever installer they claim.
const SHELL: &str = "com.android.shell";

/// API 28 added `hasSigningCertificate`.
const SIGNING_CERTIFICATE_API: u32 = 28;
/// API 30 added `getInstallSourceInfo`.
const INSTALL_SOURCE_API: u32 = 30;

/// `PackageManager` reads about another package, one platform call each; failures read as absent.
pub trait Packages {
    /// `Build.VERSION.SDK_INT`.
    fn sdk(&mut self) -> u32;
    /// Whether `package` is installed for this user.
    fn installed(&mut self, package: &str) -> bool;
    /// `hasSigningCertificate(package, sha256, CERT_INPUT_SHA256)`, from API 28.
    fn has_signing_certificate(&mut self, package: &str, sha256: &[u8; 32]) -> bool;
    /// SHA-256 of each certificate `GET_SIGNATURES` lists, before API 28.
    fn signature_digests(&mut self, package: &str) -> Vec<[u8; 32]>;
    /// `getInstallSourceInfo`, from API 30.
    fn install_source(&mut self, package: &str) -> InstallSource;
    /// `getInstallerPackageName`, before API 30.
    fn installer(&mut self, package: &str) -> Option<String>;
}

/// The installing and initiating packages `getInstallSourceInfo` reports.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstallSource {
    pub installing: Option<String>,
    pub initiating: Option<String>,
}

/// What the local install showed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Install {
    Verified,
    Missing,
    /// Not signed by Mojang's certificate.
    ForeignSigner,
    /// Not installed by a store that sells Minecraft.
    NotFromStore,
}

impl Install {
    /// A short code for the log.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Missing => "missing",
            Self::ForeignSigner => "foreign_signer",
            Self::NotFromStore => "not_from_store",
        }
    }
}

/// Checks [`PACKAGE`]'s presence, signer and installer of record through `packages`.
pub fn check(packages: &mut impl Packages) -> Install {
    let sdk = packages.sdk();
    if !packages.installed(PACKAGE) {
        return Install::Missing;
    }
    let signed = if sdk >= SIGNING_CERTIFICATE_API {
        packages.has_signing_certificate(PACKAGE, &SIGNER_SHA256)
    } else {
        let digests = packages.signature_digests(PACKAGE);
        !digests.is_empty() && digests.iter().all(|digest| *digest == SIGNER_SHA256)
    };
    if !signed {
        return Install::ForeignSigner;
    }
    let installer = if sdk >= INSTALL_SOURCE_API {
        let source = packages.install_source(PACKAGE);
        if source.initiating.as_deref() == Some(SHELL) {
            return Install::NotFromStore;
        }
        source.installing
    } else {
        packages.installer(PACKAGE)
    };
    if installer.is_some_and(|installer| STORES.contains(&installer.as_str())) {
        Install::Verified
    } else {
        Install::NotFromStore
    }
}

#[cfg(test)]
mod tests;
