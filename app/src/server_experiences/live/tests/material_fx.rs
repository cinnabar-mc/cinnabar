//! End to end, the world material showcase's real client part (examples/experiences/material-fx)
//! runs in the real developer helper: its declarations commit through the helper's contract,
//! the client validates and publishes them, its server commands update a parameter without a new
//! shader, revoke every binding so the bound entities and blocks draw as Bedrock again, and
//! restore them. The showcase's shaders' own pixels are checked by the render crate's
//! `material_fx` raster tests, and its bundle by the `material-fx` tool's tests.

use super::*;
use crate::server_experiences::world_materials::WorldMaterialPresenter;
use mod_host::helper::Helper;
use server_experience::{policy::DEVELOPER_ENV, policy::MAX_GUEST_MEMORY};
use std::{process::Command as Process, sync::Once, time::Duration};

/// The showcase's bundle id, namespace and channel, as its experience.toml declares them.
const ID: &str = "materialfx";
const CONTROL: &str = "materialfx.control";
/// The mobs the showcase restyles.
const ENTITIES: [&str; 5] = [
    "materialfx:wraith",
    "materialfx:hologram",
    "materialfx:voidwalker",
    "materialfx:lenticular",
    "materialfx:metal",
];
/// The client part's materials, and the targets bound by default (all but thermal vision).
const MATERIALS: usize = 20;
const BOUND: usize = 19;
/// The arena material, its shockwave start parameter, and thermal vision.
const ARENA: u32 = 9;
const BOOM_START: usize = 2;
const THERMAL: u32 = 5;

/// Lets this process launch developer helpers; set once, before any helper starts.
fn developer() {
    static SET: Once = Once::new();
    // SAFETY: set once, before this test starts a helper; no other test reads it concurrently.
    SET.call_once(|| unsafe { std::env::set_var(DEVELOPER_ENV, "1") });
}

/// The workspace root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A target directory of `name` beside this test binary's.
fn target(name: &str) -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .ancestors()
        .nth(3)
        .unwrap()
        .join(name)
}

/// Runs `cargo build` with `args` into `target`.
fn cargo_build(args: &[&str], target: &Path) {
    let output = Process::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root())
        .args(["build", "--locked"])
        .args(args)
        .arg("--target-dir")
        .arg(target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo build {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The showcase's signed client part as the `material-fx` tool builds it for players, with its
/// component and its signed files. Building it through the tool, not the guest alone, keeps
/// the test on the bytes the client actually loads.
fn client_part() -> server_experience::bundle::VerifiedBundle {
    let out = target("material-fx-tool");
    let output = Process::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root())
        .args(["run", "--locked", "-p", "material-fx", "--", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "the material-fx tool failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(out.join("cxb").join(format!("{ID}.cxb"))).unwrap();
    let metadata = server_experience::bundle::offline::verify(&bytes).unwrap();
    let offer = server_experience::manifest::PackageOffer {
        id: metadata.manifest.id.clone(),
        publisher_key: metadata.manifest.publisher_key.clone(),
        digest: metadata.sha256,
        bytes: metadata.bytes,
        url: String::new(),
    };
    let scope = server_experience::manifest::Scope {
        permissions: metadata.manifest.permissions.clone(),
        origins: BTreeSet::new(),
        memory_bytes: 0,
        gpu_bytes: 0,
        personal_mods: Default::default(),
    };
    server_experience::bundle::VerifiedBundle::read(
        &bytes,
        &offer,
        &scope,
        server_experience::policy::MAX_EXPANDED_BYTES,
    )
    .unwrap()
}

/// The developer helper binary.
fn helper() -> PathBuf {
    let target = target("material-fx-helper");
    cargo_build(&["-p", "mod-host", "--bin", "mod-host"], &target);
    target
        .join("debug")
        .join(format!("mod-host{}", std::env::consts::EXE_SUFFIX))
}

/// The showcase granted what its manifest asks for, its client part starting in the helper.
fn showcase() -> Live<Helper> {
    developer();
    let mut part = client_part();
    let component = part
        .take_component()
        .expect("the client part has a component");
    let files = part.take_screen_files();
    let manifest = part.manifest.clone();
    let mut live = fixture_with::<Helper>(&[ID.into()], helper(), component, |caps| {
        caps.scope.permissions = manifest.permissions.clone();
        caps.scope.memory_bytes = MAX_GUEST_MEMORY;
        caps.scope.gpu_bytes = 32 << 20;
        caps.namespace = manifest.namespace.clone();
        caps.assets = files
            .textures
            .iter()
            .map(|(path, _)| path.clone())
            .collect();
        caps.channels = manifest.channels.clone();
    });
    live.instances.get_mut(ID).unwrap().files = Arc::new(files);
    live
}

/// Drives the helper until `done` holds, advancing the callback clock each poll.
struct Driver {
    live: Live<Helper>,
    now_ms: u64,
    sequence: u64,
}

impl Driver {
    fn until(&mut self, what: &str, mut done: impl FnMut(&Live<Helper>) -> bool) {
        test_time::eventually_within(Duration::from_secs(120), what, || {
            self.now_ms += CALLBACK_INTERVAL_MS;
            self.live
                .poll(self.live.epoch, self.now_ms, true, |_| true)
                .unwrap();
            done(&self.live)
        });
    }

    /// Delivers one server command to the client part.
    fn command(&mut self, op: u16, args: [i64; 4]) {
        self.sequence += 1;
        let message = Envelope {
            version: self.live.grant.wire.version,
            session: self.live.grant.session.clone(),
            connection: self.live.grant.connection.clone(),
            subclient: 0,
            bundle: ID.into(),
            generation: INITIAL_BUNDLE_GENERATION,
            channel: CONTROL.into(),
            schema: 2,
            sequence: self.sequence,
            world_epoch: self.live.epoch,
            payload: std::iter::once(Scalar::Choice(op))
                .chain(args.map(Scalar::Integer))
                .collect(),
        };
        self.now_ms += CALLBACK_INTERVAL_MS;
        let epoch = self.live.epoch;
        self.live
            .receive(
                &ExperienceMessage::new(serde_json::to_vec(&message).unwrap()),
                self.now_ms,
                epoch,
            )
            .unwrap();
    }

    fn world(&self) -> &server_experience::world_material::WorldMaterials {
        &self.live.instances[ID].contributions.world
    }

    /// Republishes the client's drawn set; returns whether it changed.
    fn present(&self, presenter: &mut WorldMaterialPresenter) -> bool {
        presenter.rebuild(self.live.world_bundles(&std::collections::BTreeSet::new()))
    }
}

#[test]
fn the_showcase_client_part_binds_updates_revokes_and_restores_its_world_materials() {
    let mut driver = Driver {
        live: showcase(),
        now_ms: 0,
        sequence: 0,
    };
    // `init` declares as many materials as one callback's compile budget allows and binds them.
    let budget = mod_api::materials::MAX_WORLD_MATERIAL_COMPILES_PER_CALLBACK as usize;
    driver.until("the client part initializes", |live| {
        live.ready && live.instances[ID].contributions.world.materials().len() == budget
    });
    // The spawn's bind pulses declare the rest, a callback's budget at a time.
    for _ in 0..MATERIALS.div_ceil(budget) {
        driver.command(0, [0; 4]);
    }
    driver.until("every material is declared and bound", |live| {
        let world = &live.instances[ID].contributions.world;
        world.materials().len() == MATERIALS && world.bindings().len() == BOUND
    });
    let mut presenter = WorldMaterialPresenter::inline();
    assert!(driver.present(&mut presenter));
    assert_eq!(
        presenter.diagnostics().collect::<Vec<_>>(),
        Vec::<&str>::new(),
        "every shader passes the client's own validation and budgets"
    );
    for entity in ENTITIES {
        assert!(
            presenter.set().entity(entity).is_some(),
            "{entity} is bound"
        );
    }
    assert_eq!(presenter.set().blocks.len(), BOUND - ENTITIES.len());
    // A shockwave redeclares the arena with its centre and start, compiling nothing.
    let block = presenter
        .set()
        .blocks
        .keys()
        .find(|name| name.ends_with(":arena"))
        .unwrap()
        .clone();
    let before = presenter.set().block(&block).unwrap().1.clone();
    driver.command(3, [1500, -2500, 1_234_000, 0]);
    driver.until("the shockwave reaches the arena material", |live| {
        live.instances[ID].contributions.world.materials()[&ARENA]
            .params
            .get(BOOM_START)
            == Some(&1234.0)
    });
    assert!(driver.present(&mut presenter));
    let boomed = presenter.set().block(&block).unwrap().1;
    assert_eq!(boomed.key, before.key, "the shockwave compiles nothing");
    assert_eq!(
        &presenter.params().values[&boomed.key][..3],
        &[1.5, -2.5, 1234.0],
        "the shockwave travels as the arena's parameters"
    );
    // Thermal vision rebinds every mob to its own material, and back.
    driver.command(6, [1, 0, 0, 0]);
    driver.until("the mobs see in thermal", |live| {
        live.instances[ID]
            .contributions
            .world
            .bindings()
            .values()
            .filter(|binding| binding.material == THERMAL)
            .count()
            == ENTITIES.len()
    });
    driver.command(6, [0; 4]);
    driver.until("the mobs return to their materials", |live| {
        live.instances[ID]
            .contributions
            .world
            .bindings()
            .values()
            .all(|binding| binding.material != THERMAL)
    });
    // Revoking unbinds every target: each draws with its Bedrock material again.
    driver.command(1, [0; 4]);
    driver.until("every binding is removed", |live| {
        live.instances[ID].contributions.world.bindings().is_empty()
    });
    assert!(driver.present(&mut presenter));
    for target in ENTITIES {
        assert!(
            presenter.set().entity(target).is_none(),
            "{target} is Bedrock"
        );
    }
    assert!(presenter.set().blocks.is_empty());
    assert_eq!(driver.world().materials().len(), MATERIALS);
    // Restoring binds them again from the materials the client part kept.
    driver.command(0, [0; 4]);
    driver.until("the bindings return", |live| {
        live.instances[ID].contributions.world.bindings().len() == BOUND
    });
    assert!(driver.present(&mut presenter));
    assert!(presenter.set().entity(ENTITIES[0]).is_some());
}
