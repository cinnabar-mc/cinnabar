//! Android's local-install proof: Mojang's Minecraft package, signed by Mojang and installed by
//! Google Play, itself signed by Google. Best-effort: `PackageManager` metadata only, never the
//! game's files. The Amazon Appstore re-signs apps with a key it does not publish, so its copies
//! cannot pass yet.

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

/// The store whose install proves ownership.
pub const STORE: &str = "com.android.vending";

/// SHA-256 of the certificate Google signs [`STORE`] with.
pub const STORE_SIGNER_SHA256: [u8; 32] = [
    0xf0, 0xfd, 0x6c, 0x5b, 0x41, 0x0f, 0x25, 0xcb, 0x25, 0xc3, 0xb5, 0x33, 0x46, 0xc8, 0x97, 0x2f,
    0xae, 0x30, 0xf8, 0xee, 0x74, 0x11, 0xdf, 0x91, 0x04, 0x80, 0xad, 0x6b, 0x2d, 0x60, 0xdb, 0x83,
];

/// Play's web listing, which Play itself opens where it is installed.
pub const STORE_LISTING: &str =
    concat!("https://play.google.com/store/apps/details?id=", package!());

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
    /// Not installed by an authentic Google Play.
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

/// Checks [`PACKAGE`]'s presence, signer and installer through `packages`. From API 30 Play must
/// also have started the install, which rules out `adb install -i`; earlier levels cannot tell.
pub fn check(packages: &mut impl Packages) -> Install {
    let sdk = packages.sdk();
    if !packages.installed(PACKAGE) {
        return Install::Missing;
    }
    if !signed_by(packages, sdk, PACKAGE, &SIGNER_SHA256) {
        return Install::ForeignSigner;
    }
    let from_store = if sdk >= INSTALL_SOURCE_API {
        let source = packages.install_source(PACKAGE);
        source.installing.as_deref() == Some(STORE) && source.initiating.as_deref() == Some(STORE)
    } else {
        packages.installer(PACKAGE).as_deref() == Some(STORE)
    };
    if from_store && store_authentic(packages) {
        Install::Verified
    } else {
        Install::NotFromStore
    }
}

/// Whether [`STORE`] is installed and signed by Google, so links may be sent to it.
pub fn store_authentic(packages: &mut impl Packages) -> bool {
    let sdk = packages.sdk();
    packages.installed(STORE) && signed_by(packages, sdk, STORE, &STORE_SIGNER_SHA256)
}

/// Whether `package` is signed by the certificate with `sha256`, through the reads `sdk` offers.
fn signed_by(packages: &mut impl Packages, sdk: u32, package: &str, sha256: &[u8; 32]) -> bool {
    if sdk >= SIGNING_CERTIFICATE_API {
        packages.has_signing_certificate(package, sha256)
    } else {
        let digests = packages.signature_digests(package);
        !digests.is_empty() && digests.iter().all(|digest| digest == sha256)
    }
}

#[cfg(test)]
mod tests;
