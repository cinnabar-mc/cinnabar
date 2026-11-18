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
    /// Packages whose reads throw; `None` stands for the API level itself.
    unreadable: Vec<Option<&'static str>>,
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
            unreadable: Vec::new(),
            installing: Some(STORE),
            initiating: Some(STORE),
            missing_api_reads: 0,
        }
    }

    /// `package`'s signers, or the failure its reads throw.
    fn signers(&self, package: &str) -> Result<Option<&Vec<[u8; 32]>>, ReadFailed> {
        if self.unreadable.contains(&Some(package)) {
            return Err(ReadFailed);
        }
        Ok(self
            .packages
            .iter()
            .find(|(name, _)| *name == package)
            .map(|(_, signers)| signers))
    }

    /// Replaces `package`'s signers, or installs it with them.
    fn signed(mut self, package: &'static str, signers: Vec<[u8; 32]>) -> Self {
        self.packages.retain(|(name, _)| *name != package);
        self.packages.push((package, signers));
        self
    }

    fn without(mut self, package: &str) -> Self {
        self.packages.retain(|(name, _)| *name != package);
        self
    }

    fn probe(mut self) -> Probe {
        let probe = probe(&mut self);
        assert_eq!(
            self.missing_api_reads, 0,
            "read an API above level {}",
            self.sdk
        );
        probe
    }

    /// The game's install on a device whose Play is usable.
    fn check(self) -> Install {
        self.probe()
            .install
            .expect("a usable Play keeps the source")
    }
}

impl Packages for Device {
    fn sdk(&mut self) -> Result<u32, ReadFailed> {
        if self.unreadable.contains(&None) {
            return Err(ReadFailed);
        }
        Ok(self.sdk)
    }

    fn installed(&mut self, package: &str) -> Result<bool, ReadFailed> {
        Ok(self.signers(package)?.is_some())
    }

    fn enabled(&mut self, package: &str) -> Result<bool, ReadFailed> {
        Ok(self.installed(package)? && !self.disabled.contains(&package))
    }

    fn has_signing_certificate(
        &mut self,
        package: &str,
        sha256: &[u8; 32],
    ) -> Result<bool, ReadFailed> {
        self.missing_api_reads += u32::from(self.sdk < 28);
        Ok(self
            .signers(package)?
            .is_some_and(|signers| signers.contains(sha256)))
    }

    fn signature_digests(&mut self, package: &str) -> Result<Vec<[u8; 32]>, ReadFailed> {
        Ok(self.signers(package)?.cloned().unwrap_or_default())
    }

    fn install_source(&mut self, package: &str) -> Result<InstallSource, ReadFailed> {
        self.missing_api_reads += u32::from(self.sdk < 30);
        if package != PACKAGE || !self.installed(package)? {
            return Ok(InstallSource::default());
        }
        Ok(InstallSource {
            installing: self.installing.map(str::to_owned),
            initiating: self.initiating.map(str::to_owned),
        })
    }

    fn installer(&mut self, package: &str) -> Result<Option<String>, ReadFailed> {
        if package != PACKAGE || !self.installed(package)? {
            return Ok(None);
        }
        Ok(self.installing.map(str::to_owned))
    }
}

const API_LEVELS: [u32; 6] = [26, 27, 28, 29, 30, 35];

/// Turns a Play-installed device into one case's device.
type Setup = fn(Device) -> Device;

/// Every Play state against every game install: only a clean negative about Play turns the gate
/// off, an unreadable Play keeps it on and is unverifiable whatever the game shows, and only a
/// genuine Play install of the game passes.
#[test]
fn each_play_state_and_game_install_decides_the_gate() {
    use PlayState::*;
    let plays: [(PlayState, Setup); 6] = [
        (Usable, |device| device),
        (Absent, |device| device.without(STORE)),
        (Disabled, |device| Device {
            disabled: vec![STORE],
            ..device
        }),
        (NotGenuine, |device| {
            device.signed(STORE, vec![OTHER_SIGNER])
        }),
        (Unreadable, |device| Device {
            unreadable: vec![Some(STORE)],
            ..device
        }),
        (Unreadable, |device| Device {
            unreadable: vec![None],
            ..device
        }),
    ];
    let games: [(&str, Setup); 5] = [
        ("genuine", |device| device),
        ("missing", |device| device.without(PACKAGE)),
        ("sideloaded", |device| Device {
            installing: Some("com.android.packageinstaller"),
            initiating: Some("com.android.packageinstaller"),
            ..device
        }),
        ("resigned", |device| {
            device.signed(PACKAGE, vec![OTHER_SIGNER])
        }),
        ("unreadable", |device| {
            let mut device = device;
            device.unreadable.push(Some(PACKAGE));
            device
        }),
    ];
    let android = Sources {
        local_install: Some(Store::Android),
        account: false,
    };
    for sdk in API_LEVELS {
        for (play, set_play) in plays {
            for (game, set_game) in games {
                let probe = set_game(set_play(Device::play_install(sdk))).probe();
                let case = format!("{play:?} Play, {game} game, API {sdk}");
                assert_eq!(probe.play, play, "{case}");
                let local = probe.local();
                let gated = android.with_store_usable(local.store_usable).gated();
                let expected = match (play, game) {
                    (Absent | Disabled | NotGenuine, _) => None,
                    // An unreadable Play is unverifiable whatever the game shows.
                    (Unreadable, _) | (Usable, "unreadable") => Some(LocalInstall::Unreadable),
                    (Usable, "genuine") => Some(LocalInstall::Found),
                    (Usable, _) => Some(LocalInstall::NotFound),
                };
                assert_eq!(gated, expected.is_some(), "{case}: gate");
                if let Some(install) = expected {
                    assert_eq!(local.install, install, "{case}: install");
                }
                assert_eq!(
                    local.store_app,
                    (play == Usable).then_some(STORE),
                    "{case}: store app"
                );
            }
        }
    }
}

#[test]
fn only_the_full_game_counts() {
    for sdk in API_LEVELS {
        let none = Device::play_install(sdk).without(PACKAGE);
        assert_eq!(none.clone().check(), Install::Missing, "API {sdk}");
        let trial = none.signed(TRIAL, vec![SIGNER_SHA256]);
        assert_eq!(trial.check(), Install::Missing, "trial on API {sdk}");
    }
}

/// Before API 28 every listed signer must be Mojang's.
#[test]
fn every_legacy_signer_must_be_mojangs() {
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
