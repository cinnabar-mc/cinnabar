//! Android's local-install proof: Mojang's Minecraft package, signed by Mojang and installed by
//! Google Play, itself signed by Google. Best-effort: `PackageManager` metadata only, never the
//! game's files. Devices where Play is cleanly absent, disabled or not genuine have no such proof,
//! so the source is offered only where [`probe`] cannot rule Play out. The Amazon Appstore re-signs
//! apps with a key it does not publish, so its copies cannot pass yet.

use super::{LocalInstall, LocalProbe};

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

/// A `PackageManager` read that failed for any reason but the package being absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadFailed;

/// `PackageManager` reads about another package, one platform call each.
pub trait Packages {
    /// `Build.VERSION.SDK_INT`.
    fn sdk(&mut self) -> Result<u32, ReadFailed>;
    /// Whether `package` is installed for this user, enabled or not; absent is `Ok(false)`.
    fn installed(&mut self, package: &str) -> Result<bool, ReadFailed>;
    /// Whether `package` is enabled: `ApplicationInfo.enabled`, and its enabled setting is not
    /// disabled, disabled by the user or disabled until used.
    fn enabled(&mut self, package: &str) -> Result<bool, ReadFailed>;
    /// `hasSigningCertificate(package, sha256, CERT_INPUT_SHA256)`, from API 28.
    fn has_signing_certificate(
        &mut self,
        package: &str,
        sha256: &[u8; 32],
    ) -> Result<bool, ReadFailed>;
    /// SHA-256 of each certificate `GET_SIGNATURES` lists, before API 28.
    fn signature_digests(&mut self, package: &str) -> Result<Vec<[u8; 32]>, ReadFailed>;
    /// `getInstallSourceInfo`, from API 30.
    fn install_source(&mut self, package: &str) -> Result<InstallSource, ReadFailed>;
    /// `getInstallerPackageName`, before API 30.
    fn installer(&mut self, package: &str) -> Result<Option<String>, ReadFailed>;
}

/// The installing and initiating packages `getInstallSourceInfo` reports.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstallSource {
    pub installing: Option<String>,
    pub initiating: Option<String>,
}

/// What Google Play is on this device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayState {
    /// Installed for this user, enabled and signed by Google.
    Usable,
    Absent,
    Disabled,
    /// Not signed by Google's certificate.
    NotGenuine,
    /// A read failed, so nothing rules Play out.
    Unreadable,
}

impl PlayState {
    /// Only a clean answer that Play is absent, disabled or not genuine removes the source.
    #[must_use]
    pub const fn keeps_source(self) -> bool {
        matches!(self, Self::Usable | Self::Unreadable)
    }

    /// A short code for the log.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Usable => "usable",
            Self::Absent => "absent",
            Self::Disabled => "disabled",
            Self::NotGenuine => "not_genuine",
            Self::Unreadable => "unreadable",
        }
    }
}

/// What the game's install showed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Install {
    Verified,
    Missing,
    /// Not signed by Mojang's certificate.
    ForeignSigner,
    /// Not installed by a genuine Google Play.
    NotFromStore,
    /// A read failed before the check could finish.
    Unreadable,
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
            Self::Unreadable => "unreadable",
        }
    }
}

/// One look at Play and, where Play keeps the source, the game's install.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Probe {
    pub play: PlayState,
    /// `None` where Play removed the source, so the game was not checked.
    pub install: Option<Install>,
}

impl Probe {
    /// The platform-neutral view the gate decides on.
    #[must_use]
    pub const fn local(self) -> LocalProbe {
        LocalProbe {
            store_usable: self.play.keeps_source(),
            install: match self.install {
                Some(Install::Verified) => LocalInstall::Found,
                Some(Install::Unreadable) => LocalInstall::Unreadable,
                Some(Install::Missing | Install::ForeignSigner | Install::NotFromStore) | None => {
                    LocalInstall::NotFound
                }
            },
            store_app: match self.play {
                PlayState::Usable => Some(STORE),
                PlayState::Absent
                | PlayState::Disabled
                | PlayState::NotGenuine
                | PlayState::Unreadable => None,
            },
        }
    }
}

/// Probes Play, then the game where Play is usable; an unreadable Play is unreadable whatever
/// the game shows. The game passes only when it is [`PACKAGE`], signed by Mojang and installed by
/// a usable Play; from API 30 Play must also have started the install, which rules out
/// `adb install -i`, while earlier levels cannot tell.
pub fn probe(packages: &mut impl Packages) -> Probe {
    let play = play_state(packages).unwrap_or(PlayState::Unreadable);
    let install = match play {
        PlayState::Usable => Some(check(packages).unwrap_or(Install::Unreadable)),
        // Until Play can be read it is unknown whether the gate applies, so the game's own
        // answer would mislead; Try again probes again.
        PlayState::Unreadable => Some(Install::Unreadable),
        PlayState::Absent | PlayState::Disabled | PlayState::NotGenuine => None,
    };
    Probe { play, install }
}

fn play_state(packages: &mut impl Packages) -> Result<PlayState, ReadFailed> {
    let sdk = packages.sdk()?;
    Ok(if !packages.installed(STORE)? {
        PlayState::Absent
    } else if !packages.enabled(STORE)? {
        PlayState::Disabled
    } else if !signed_by(packages, sdk, STORE, &STORE_SIGNER_SHA256)? {
        PlayState::NotGenuine
    } else {
        PlayState::Usable
    })
}

/// The game's install on a device whose Play is usable.
fn check(packages: &mut impl Packages) -> Result<Install, ReadFailed> {
    let sdk = packages.sdk()?;
    if !packages.installed(PACKAGE)? {
        return Ok(Install::Missing);
    }
    if !signed_by(packages, sdk, PACKAGE, &SIGNER_SHA256)? {
        return Ok(Install::ForeignSigner);
    }
    let from_store = if sdk >= INSTALL_SOURCE_API {
        let source = packages.install_source(PACKAGE)?;
        source.installing.as_deref() == Some(STORE) && source.initiating.as_deref() == Some(STORE)
    } else {
        packages.installer(PACKAGE)?.as_deref() == Some(STORE)
    };
    Ok(if from_store {
        Install::Verified
    } else {
        Install::NotFromStore
    })
}

/// Whether `package` is signed by the certificate with `sha256`, through the reads `sdk` offers.
fn signed_by(
    packages: &mut impl Packages,
    sdk: u32,
    package: &str,
    sha256: &[u8; 32],
) -> Result<bool, ReadFailed> {
    if sdk >= SIGNING_CERTIFICATE_API {
        packages.has_signing_certificate(package, sha256)
    } else {
        let digests = packages.signature_digests(package)?;
        Ok(!digests.is_empty() && digests.iter().all(|digest| digest == sha256))
    }
}

#[cfg(test)]
mod tests;
