//! End to end, the retained-drawing material showcase mod (examples/mods/material-showcase)
//! loads through the real mod path with the grants it needs: its two `create-material` shaders
//! validate and are retained, the first gameplay frame places four waypoint beams and an
//! outline drawn with them, later frames move only the outline, a drawing that fails is not
//! retried every frame, and without `render_material` no custom material is created.

use std::path::{Path, PathBuf};

use mod_host::{GameplaySnapshot, GameplayVector3, ModGrants, ModHost};

/// The showcase mod, built for wasm32 and packed by `mod-host pack --grants` with its grant
/// request, as a component in a fresh directory that lives as long as the returned guard. Tests
/// run in parallel processes, so the build and the read of its `.wasm` hold a cross-process lock,
/// and each test packs its own copy of the component rather than one another test may be
/// rewriting.
fn component() -> (tempfile::TempDir, PathBuf) {
    let exe = std::env::current_exe().unwrap();
    let target = exe
        .ancestors()
        .nth(3)
        .unwrap()
        .join("material-showcase-mod");
    std::fs::create_dir_all(&target).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(target.join("fixture.lock"))
        .unwrap();
    lock.lock().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output =
        std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .current_dir(&root)
            .args(["build", "--locked", "--target", "wasm32-unknown-unknown"])
            .args(["-p", "material-showcase-mod", "--target-dir"])
            .arg(&target)
            .output()
            .unwrap();
    assert!(
        output.status.success(),
        "building the showcase mod failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let module =
        std::fs::read(target.join("wasm32-unknown-unknown/debug/material_showcase_mod.wasm"))
            .unwrap();
    drop(lock);
    let dir = tempfile::tempdir().unwrap();
    let core = dir.path().join("material_showcase_mod.wasm");
    std::fs::write(&core, module).unwrap();
    let path = dir.path().join("material_showcase.component.wasm");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mod-host"))
        .arg("pack")
        .arg("--grants")
        .arg(root.join("examples/mods/material-showcase/grants.json"))
        .arg(&core)
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "packing the showcase mod failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (dir, path)
}

/// A gameplay frame with the eye at `x`.
fn frame(x: f32) -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x,
            y: 65.62,
            z: 0.5,
        },
        yaw: 0.0,
        pitch: 0.0,
        attack_held: false,
        frame_seconds: 1.0 / 60.0,
        players: Vec::new(),
    }
}

fn grants(render_material: bool) -> ModGrants {
    ModGrants {
        players: true,
        hud: true,
        world_draw: true,
        render_material,
        ..Default::default()
    }
}

#[test]
fn the_showcase_mod_draws_beams_and_a_moving_outline_with_its_own_materials() {
    let (_dir, path) = component();
    let mut host = ModHost::load_with_grants(&path, grants(true)).unwrap();
    assert_eq!(
        host.label(),
        Some("Material showcase: beams and outline ready")
    );
    host.frame_with_gameplay(false, Some(frame(0.5))).unwrap();
    let retained = std::sync::Arc::clone(&host.render().0.retained[0]);
    assert_eq!(
        retained.materials.len(),
        2,
        "both shaders validate and are retained"
    );
    assert_eq!(
        retained.instances.len(),
        5,
        "four beams and the outline: {:?}",
        host.label()
    );
    assert!(
        retained
            .instances
            .values()
            .all(|instance| instance.material.is_some()),
        "every drawing uses a showcase material"
    );
    // Walking moves the outline only.
    host.frame_with_gameplay(false, Some(frame(3.5))).unwrap();
    let moved = std::sync::Arc::clone(&host.render().0.retained[0]);
    assert_eq!(moved.instances.len(), 5);
    assert_eq!(moved.geometry_revision, retained.geometry_revision);
    assert_eq!(moved.material_revision, retained.material_revision);
    assert_ne!(moved.instance_revision, retained.instance_revision);
    assert!(host.is_active());
}

#[test]
fn a_beam_that_cannot_be_placed_is_not_retried_every_frame() {
    let (_dir, path) = component();
    let mut host = ModHost::load_with_grants(&path, grants(true)).unwrap();
    // Near the coordinate limit the beam six blocks further out cannot be drawn; the other
    // three beams and the outline still are, once.
    let edge = mod_api::MAX_PRIMITIVE_COORDINATE - 4.0;
    for _ in 0..3 {
        host.frame_with_gameplay(false, Some(frame(edge))).unwrap();
        assert_eq!(
            host.render().0.retained[0].instances.len(),
            4,
            "{:?}",
            host.label()
        );
    }
    assert!(
        host.label().is_some_and(
            |label| label.starts_with("Material showcase:") && !label.contains("ready")
        ),
        "{:?}",
        host.label()
    );
}

#[test]
fn without_the_render_material_grant_the_showcase_makes_no_material() {
    let (_dir, path) = component();
    let mut host = ModHost::load_with_grants(&path, grants(false)).unwrap();
    assert!(
        host.label().is_some_and(
            |label| label.starts_with("Material showcase:") && !label.contains("ready")
        ),
        "{:?}",
        host.label()
    );
    host.frame_with_gameplay(false, Some(frame(0.5))).unwrap();
    assert!(
        host.render()
            .0
            .retained
            .iter()
            .all(|resources| resources.materials.is_empty())
    );
}
