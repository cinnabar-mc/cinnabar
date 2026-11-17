use super::*;
use crate::ownership::{Sources, Store};

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
    /// Installed packages the user or system disabled.
    disabled: Vec<&'static str>,
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
            disabled: Vec::new(),
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

    fn enabled(&mut self, package: &str) -> bool {
        self.installed(package) && !self.disabled.contains(&package)
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

/// The sources a device offers on Android, after probing Play.
fn device_sources(device: &mut Device) -> Sources {
    let android = Sources {
        local_install: Some(Store::Android),
        account: false,
    };
    android.with_store_usable(store_available(device).is_ok())
}

/// Without an enabled, genuine Play the device has no proof source, so the gate is off there.
#[test]
fn a_device_without_a_usable_play_is_ungated() {
    for sdk in API_LEVELS {
        let mut absent = Device::play_install(sdk);
        absent.packages.retain(|(name, _)| *name != STORE);
        let disabled = Device {
            disabled: vec![STORE],
            ..Device::play_install(sdk)
        };
        let impostor = Device::play_install(sdk).signed(STORE, vec![OTHER_SIGNER]);
        let unsigned = Device::play_install(sdk).signed(STORE, vec![]);
        for (mut device, reason) in [
            (absent, StoreUnavailable::Absent),
            (disabled, StoreUnavailable::Disabled),
            (impostor, StoreUnavailable::NotGenuine),
            (unsigned, StoreUnavailable::NotGenuine),
        ] {
            assert_eq!(store_available(&mut device), Err(reason), "API {sdk}");
            let sources = device_sources(&mut device);
            assert!(!sources.gated(), "{reason:?} on API {sdk}");
            assert_eq!(sources.store_app(), None);
        }
    }
}

/// With an enabled, genuine Play the gate is on, and only a genuine Play install of the game
/// passes it.
#[test]
fn a_device_with_play_is_gated_on_the_game_install() {
    for sdk in API_LEVELS {
        let mut device = Device::play_install(sdk);
        let sources = device_sources(&mut device);
        assert!(sources.gated(), "API {sdk}");
        assert_eq!(sources.store_app(), Some(STORE));
        assert_eq!(device.check(), Install::Verified, "API {sdk}");

        let mut missing = Device::play_install(sdk);
        missing.packages.retain(|(name, _)| *name != PACKAGE);
        let sideloaded = Device {
            installing: Some("com.android.packageinstaller"),
            initiating: Some("com.android.packageinstaller"),
            ..Device::play_install(sdk)
        };
        let resigned = Device::play_install(sdk).signed(PACKAGE, vec![OTHER_SIGNER]);
        for (mut device, refused) in [
            (missing, Install::Missing),
            (sideloaded, Install::NotFromStore),
            (resigned, Install::ForeignSigner),
        ] {
            assert!(device_sources(&mut device).gated(), "API {sdk}");
            assert_eq!(device.check(), refused, "API {sdk}");
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
fn get_minecraft_opens_the_full_game_on_play() {
    assert!(STORE_LISTING.starts_with("https://play.google.com/"));
    assert!(STORE_LISTING.ends_with(&format!("id={PACKAGE}")));
}

/// Android 11+ hides other packages, and installers it can't see, unless the manifest declares
/// them.
#[test]
fn the_manifest_can_see_the_game_and_its_store() {
    let source = include_str!("../../../../../packaging/android/AndroidManifest.xml");
    // Commented-out declarations don't count, and spacing doesn't matter.
    let manifest: String = source
        .split("<!--")
        .enumerate()
        .map(|(index, part)| {
            if index == 0 {
                part
            } else {
                part.split_once("-->").map_or("", |(_, rest)| rest)
            }
        })
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ");
    let queries = manifest
        .split_once("<queries>")
        .and_then(|(_, rest)| rest.split_once("</queries>"))
        .map_or("", |(queries, _)| queries);
    for package in [PACKAGE, STORE] {
        let query = format!("<package android:name=\"{package}\"");
        assert!(queries.contains(&query), "{package} is not under <queries>");
    }
    assert!(!manifest.contains("QUERY_ALL_PACKAGES"));
}
