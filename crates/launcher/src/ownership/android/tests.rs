use super::*;

const TRIAL: &str = "com.mojang.minecrafttrialpe";
const AMAZON: &str = "com.amazon.venezia";
const SHELL: &str = "com.android.shell";
const OTHER_SIGNER: [u8; 32] = [7; 32];

/// A device's installed packages and the game's install source, answering reads by API level.
#[derive(Clone)]
struct Device {
    sdk: u32,
    /// Each installed package and its signers.
    packages: Vec<(&'static str, Vec<[u8; 32]>)>,
    installing: Option<&'static str>,
    initiating: Option<&'static str>,
    /// Reads this device's API level lacks, which a correct check never makes.
    missing_api_reads: u32,
}

impl Device {
    fn play_install(sdk: u32) -> Self {
        Self {
            sdk,
            packages: vec![
                (PACKAGE, vec![SIGNER_SHA256]),
                (STORE, vec![STORE_SIGNER_SHA256]),
            ],
            installing: Some(STORE),
            initiating: Some(STORE),
            missing_api_reads: 0,
        }
    }

    fn signers(&self, package: &str) -> Option<&Vec<[u8; 32]>> {
        self.packages
            .iter()
            .find(|(name, _)| *name == package)
            .map(|(_, signers)| signers)
    }

    /// Replaces `package`'s signers, or installs it with them.
    fn signed(mut self, package: &'static str, signers: Vec<[u8; 32]>) -> Self {
        self.packages.retain(|(name, _)| *name != package);
        self.packages.push((package, signers));
        self
    }

    fn check(mut self) -> Install {
        let install = check(&mut self);
        assert_eq!(
            self.missing_api_reads, 0,
            "read an API above level {}",
            self.sdk
        );
        install
    }
}

impl Packages for Device {
    fn sdk(&mut self) -> u32 {
        self.sdk
    }

    fn installed(&mut self, package: &str) -> bool {
        self.signers(package).is_some()
    }

    fn has_signing_certificate(&mut self, package: &str, sha256: &[u8; 32]) -> bool {
        self.missing_api_reads += u32::from(self.sdk < 28);
        self.signers(package)
            .is_some_and(|signers| signers.contains(sha256))
    }

    fn signature_digests(&mut self, package: &str) -> Vec<[u8; 32]> {
        self.signers(package).cloned().unwrap_or_default()
    }

    fn install_source(&mut self, package: &str) -> InstallSource {
        self.missing_api_reads += u32::from(self.sdk < 30);
        if package != PACKAGE || !self.installed(package) {
            return InstallSource::default();
        }
        InstallSource {
            installing: self.installing.map(str::to_owned),
            initiating: self.initiating.map(str::to_owned),
        }
    }

    fn installer(&mut self, package: &str) -> Option<String> {
        if package != PACKAGE || !self.installed(package) {
            return None;
        }
        self.installing.map(str::to_owned)
    }
}

const API_LEVELS: [u32; 6] = [26, 27, 28, 29, 30, 35];

#[test]
fn a_play_install_signed_by_mojang_passes_at_every_api_level() {
    for sdk in API_LEVELS {
        assert_eq!(
            Device::play_install(sdk).check(),
            Install::Verified,
            "API {sdk}"
        );
    }
}

#[test]
fn only_the_full_game_counts() {
    for sdk in API_LEVELS {
        let mut none = Device::play_install(sdk);
        none.packages.retain(|(name, _)| *name != PACKAGE);
        assert_eq!(none.clone().check(), Install::Missing, "API {sdk}");
        let trial = none.signed(TRIAL, vec![SIGNER_SHA256]);
        assert_eq!(trial.check(), Install::Missing, "trial on API {sdk}");
    }
}

#[test]
fn another_signer_fails() {
    for sdk in API_LEVELS {
        let resigned = Device::play_install(sdk).signed(PACKAGE, vec![OTHER_SIGNER]);
        assert_eq!(resigned.check(), Install::ForeignSigner, "API {sdk}");
    }
    // Before API 28 every listed signer must be Mojang's.
    for signers in [vec![], vec![SIGNER_SHA256, OTHER_SIGNER]] {
        let device = Device::play_install(27).signed(PACKAGE, signers);
        assert_eq!(device.check(), Install::ForeignSigner);
    }
}

#[test]
fn other_installers_fail() {
    for sdk in API_LEVELS {
        for installer in [
            None,
            Some("com.android.packageinstaller"),
            Some(SHELL),
            Some(AMAZON),
        ] {
            let device = Device {
                installing: installer,
                initiating: installer,
                ..Device::play_install(sdk)
            };
            assert_eq!(
                device.check(),
                Install::NotFromStore,
                "{installer:?} on API {sdk}"
            );
        }
    }
}

/// An app that takes Play's package name on a device without Play is not Play.
#[test]
fn an_impostor_store_fails() {
    for sdk in API_LEVELS {
        for signers in [vec![OTHER_SIGNER], vec![]] {
            let device = Device::play_install(sdk).signed(STORE, signers);
            assert_eq!(device.check(), Install::NotFromStore, "API {sdk}");
        }
    }
}

/// `adb install -i com.android.vending` claims Play as installer; from API 30 the initiator is
/// the shell, or none on Android 11–13. Before API 30 nothing tells them apart.
#[test]
fn a_shell_install_claiming_play_fails_where_the_initiator_is_known() {
    for initiating in [Some(SHELL), None] {
        let spoofed = |sdk| Device {
            initiating,
            ..Device::play_install(sdk)
        };
        for sdk in [30, 33, 35] {
            assert_eq!(
                spoofed(sdk).check(),
                Install::NotFromStore,
                "{initiating:?} on API {sdk}"
            );
        }
        for sdk in [26, 29] {
            assert_eq!(spoofed(sdk).check(), Install::Verified, "API {sdk}");
        }
    }
}

#[test]
fn get_minecraft_opens_the_play_listing_first() {
    assert!(STORE_LISTING[0].url.starts_with("market://"));
    assert_eq!(STORE_LISTING[0].app, Some(STORE));
    assert!(STORE_LISTING.iter().all(|link| link.url.ends_with(PACKAGE)));
}

/// Android 11+ hides other packages, and installers it can't see, unless the manifest declares
/// them.
#[test]
fn the_manifest_can_see_the_game_and_its_store() {
    let manifest = include_str!("../../../../../packaging/android/AndroidManifest.xml");
    for package in [PACKAGE, STORE] {
        let query = format!("<package android:name=\"{package}\" />");
        assert!(manifest.contains(&query), "{package}");
    }
    assert!(!manifest.contains("QUERY_ALL_PACKAGES"));
}
