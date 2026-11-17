use super::*;

const TRIAL: &str = "com.mojang.minecrafttrialpe";
const OTHER_SIGNER: [u8; 32] = [7; 32];

/// A device with one package installed, answering each read by API level.
#[derive(Clone)]
struct Device {
    sdk: u32,
    package: &'static str,
    signers: Vec<[u8; 32]>,
    installing: Option<&'static str>,
    initiating: Option<&'static str>,
    /// API reads this device's level lacks, which a correct check never makes.
    missing_api_reads: u32,
}

impl Device {
    fn store_install(sdk: u32) -> Self {
        Self {
            sdk,
            package: PACKAGE,
            signers: vec![SIGNER_SHA256],
            installing: Some("com.android.vending"),
            initiating: Some("com.android.vending"),
            missing_api_reads: 0,
        }
    }

    fn present(&self, package: &str) -> bool {
        package == self.package
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
        self.present(package)
    }

    fn has_signing_certificate(&mut self, package: &str, sha256: &[u8; 32]) -> bool {
        self.missing_api_reads += u32::from(self.sdk < 28);
        self.present(package) && self.signers.contains(sha256)
    }

    fn signature_digests(&mut self, package: &str) -> Vec<[u8; 32]> {
        if self.present(package) {
            self.signers.clone()
        } else {
            Vec::new()
        }
    }

    fn install_source(&mut self, package: &str) -> InstallSource {
        self.missing_api_reads += u32::from(self.sdk < 30);
        if !self.present(package) {
            return InstallSource::default();
        }
        InstallSource {
            installing: self.installing.map(str::to_owned),
            initiating: self.initiating.map(str::to_owned),
        }
    }

    fn installer(&mut self, package: &str) -> Option<String> {
        self.installing
            .filter(|_| self.present(package))
            .map(str::to_owned)
    }
}

const API_LEVELS: [u32; 5] = [26, 27, 28, 30, 35];

#[test]
fn a_store_install_signed_by_mojang_passes_at_every_api_level() {
    for sdk in API_LEVELS {
        for store in STORES {
            let device = Device {
                installing: Some(store),
                initiating: Some(store),
                ..Device::store_install(sdk)
            };
            assert_eq!(device.check(), Install::Verified, "{store} on API {sdk}");
        }
    }
}

#[test]
fn only_the_full_game_counts() {
    for sdk in API_LEVELS {
        let none = Device {
            package: "com.example.other",
            ..Device::store_install(sdk)
        };
        assert_eq!(none.check(), Install::Missing, "API {sdk}");
        let trial = Device {
            package: TRIAL,
            ..Device::store_install(sdk)
        };
        assert_eq!(trial.check(), Install::Missing, "trial on API {sdk}");
    }
}

#[test]
fn another_signer_fails() {
    for sdk in API_LEVELS {
        let resigned = Device {
            signers: vec![OTHER_SIGNER],
            ..Device::store_install(sdk)
        };
        assert_eq!(resigned.check(), Install::ForeignSigner, "API {sdk}");
    }
    // Before API 28 every listed signer must be Mojang's.
    for signers in [vec![], vec![SIGNER_SHA256, OTHER_SIGNER]] {
        let device = Device {
            signers,
            ..Device::store_install(27)
        };
        assert_eq!(device.check(), Install::ForeignSigner);
    }
}

#[test]
fn sideloads_and_unknown_installers_fail() {
    for sdk in API_LEVELS {
        for installer in [None, Some("com.android.packageinstaller"), Some(SHELL)] {
            let device = Device {
                installing: installer,
                initiating: installer,
                ..Device::store_install(sdk)
            };
            assert_eq!(
                device.check(),
                Install::NotFromStore,
                "{installer:?} on API {sdk}"
            );
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
            ..Device::store_install(sdk)
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
    assert!(STORE_LISTING[0].starts_with("market://"));
    assert!(STORE_LISTING.iter().all(|url| url.ends_with(PACKAGE)));
}

/// Android 11+ hides other packages, and installers it can't see, unless the manifest declares
/// them.
#[test]
fn the_manifest_can_see_the_game_and_its_stores() {
    let manifest = include_str!("../../../../../packaging/android/AndroidManifest.xml");
    for package in std::iter::once(PACKAGE).chain(STORES) {
        let query = format!("<package android:name=\"{package}\" />");
        assert!(manifest.contains(&query), "{package}");
    }
    assert!(!manifest.contains("QUERY_ALL_PACKAGES"));
}
