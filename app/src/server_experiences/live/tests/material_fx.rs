//! End to end, the world material showcase's real client part (examples/experiences/material-fx)
//! runs in the real developer helper: its declarations commit through the helper's contract,
//! the client validates and publishes them, its server commands update a parameter without a new
//! shader, revoke every binding so the bound entities and blocks draw as Bedrock again, and
//! restore them. The showcase's shaders' own pixels are checked by the render crate's
//! `material_fx` raster tests, and its bundle by the `material-fx` tool's tests.

use super::*;
use crate::server_experiences::world_materials::WorldMaterialPresenter;
use mod_host::helper::Helper;
use server_experience::{policy::DEVELOPER_ENV, policy::MAX_GUEST_MEMORY, screen::Files};
use std::{process::Command as Process, sync::Once, time::Duration};

/// The showcase's bundle id, namespace and channel, as its experience.toml declares them.
const ID: &str = "materialfx";
const CONTROL: &str = "materialfx.control";
/// The targets the showcase restyles, by material id.
const ENTITIES: [&str; 3] = [
    "materialfx:wraith",
    "materialfx:hologram",
    "materialfx:sentinel",
];
const BLOCKS: [&str; 2] = ["materialfx:energy", "materialfx:portal"];
/// The client part's signed textures, as its experience.toml lists them.
const TEXTURES: [&str; 2] = ["textures/noise.png", "textures/stars.png"];
/// The status material's id and its flash parameter.
const STATUS: u32 = 3;
const FLASH: usize = 4;

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

/// The showcase client part, built for wasm32 and turned into a component.
fn client_component() -> Vec<u8> {
    let target = target("material-fx-guests");
    cargo_build(
        &[
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            "experience-material-fx-client",
        ],
        &target,
    );
    let module = std::fs::read(
        target.join("wasm32-unknown-unknown/debug/experience_material_fx_client.wasm"),
    )
    .unwrap();
    wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
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

/// A PNG of one opaque grey texel.
fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([128, 128, 128, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// The showcase granted what its manifest asks for, its client part starting in the helper.
fn showcase() -> Live<Helper> {
    developer();
    let mut live = fixture_with::<Helper>(&[ID.into()], helper(), client_component(), |caps| {
        caps.scope.permissions =
            BTreeSet::from([Permission::Messaging, Permission::RenderWorldMaterial]);
        caps.scope.memory_bytes = MAX_GUEST_MEMORY;
        caps.scope.gpu_bytes = 32 << 20;
        caps.namespace = Some(ID.into());
        caps.assets = TEXTURES.iter().map(|path| (*path).to_owned()).collect();
        caps.channels = vec![Channel {
            id: CONTROL.into(),
            schema: 1,
            direction: Direction::ToClient,
            fields: vec![
                Field::Choice { variants: 5 },
                Field::Integer { min: -1, max: 1000 },
            ],
        }];
    });
    live.instances.get_mut(ID).unwrap().files = Arc::new(Files {
        namespace: ID.into(),
        templates: Default::default(),
        textures: TEXTURES
            .iter()
            .map(|path| ((*path).to_owned(), png()))
            .collect(),
    });
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
            self.live.poll(self.live.epoch, self.now_ms).unwrap();
            done(&self.live)
        });
    }

    /// Delivers one server command to the client part.
    fn command(&mut self, op: u16, value: i64) {
        self.sequence += 1;
        let message = Envelope {
            version: self.live.grant.wire.version,
            session: self.live.grant.session.clone(),
            connection: self.live.grant.connection.clone(),
            subclient: 0,
            bundle: ID.into(),
            generation: INITIAL_BUNDLE_GENERATION,
            channel: CONTROL.into(),
            schema: 1,
            sequence: self.sequence,
            world_epoch: self.live.epoch,
            payload: vec![Scalar::Choice(op), Scalar::Integer(value)],
        };
        self.now_ms += CALLBACK_INTERVAL_MS;
        self.live
            .receive(&serde_json::to_vec(&message).unwrap(), self.now_ms)
            .unwrap();
    }

    fn world(&self) -> &server_experience::world_material::WorldMaterials {
        &self.live.instances[ID].contributions.world
    }

    /// Republishes the client's drawn set; returns whether it changed.
    fn present(&self, presenter: &mut WorldMaterialPresenter) -> bool {
        presenter.rebuild(self.live.world_bundles())
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
        live.ready && live.instances[ID].contributions.world.bindings.len() == budget
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
    assert!(presenter.set().block(BLOCKS[0]).is_some());
    // The spawn command binds everything, declaring the material `init` had no budget for.
    driver.command(0, 0);
    driver.until("every material is bound", |live| {
        live.instances[ID].contributions.world.bindings.len() == ENTITIES.len() + BLOCKS.len()
    });
    assert!(driver.present(&mut presenter));
    assert!(
        presenter.set().block(BLOCKS[1]).is_some(),
        "the portal is bound"
    );
    // A damage flash redeclares the status material with new parameters, keeping its shader.
    let before = presenter.set().entity(ENTITIES[2]).unwrap().1.clone();
    driver.command(2, 1000);
    driver.until("the flash reaches the status material", |live| {
        live.instances[ID].contributions.world.materials[&STATUS]
            .params
            .get(FLASH)
            == Some(&1.0)
    });
    assert!(driver.present(&mut presenter));
    let flashed = presenter.set().entity(ENTITIES[2]).unwrap().1;
    assert_eq!(flashed.key, before.key, "the flash compiles nothing");
    assert_eq!(flashed.params[FLASH], 1.0);
    // Revoking unbinds every target: each draws with its Bedrock material again.
    driver.command(1, 0);
    driver.until("every binding is removed", |live| {
        live.instances[ID].contributions.world.bindings.is_empty()
    });
    assert!(driver.present(&mut presenter));
    for target in ENTITIES {
        assert!(
            presenter.set().entity(target).is_none(),
            "{target} is Bedrock"
        );
    }
    for target in BLOCKS {
        assert!(
            presenter.set().block(target).is_none(),
            "{target} is Bedrock"
        );
    }
    assert_eq!(
        driver.world().materials.len(),
        ENTITIES.len() + BLOCKS.len()
    );
    // Restoring binds them again from the materials the client part kept.
    driver.command(0, 0);
    driver.until("the bindings return", |live| {
        live.instances[ID].contributions.world.bindings.len() == ENTITIES.len() + BLOCKS.len()
    });
    assert!(driver.present(&mut presenter));
    assert!(presenter.set().entity(ENTITIES[0]).is_some());
}
