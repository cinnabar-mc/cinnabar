use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::{test_support::Dir, tests::installed_layout};

/// A kit whose compiler reports `world` stale until the stamp names `generation`; a build runs
/// `build`, a shell fragment with `$out` set.
fn write_kit(kit: &Path, generation: &str, build: &str) {
    let script = format!(
        "#!/bin/sh\nset -eu\ncheck=0\nwhile [ \"$#\" -gt 0 ]; do\n\
         case \"$1\" in\n--out) out=\"$2\"; shift;;\n--check) check=1;;\nesac\nshift\ndone\n\
         if [ \"$check\" = 1 ]; then\n\
         if [ \"$(cat \"$out/{STAMP_FILE}\" 2>/dev/null || true)\" = '{generation}' ]; then\n\
         echo '{{\"stale\":[],\"needs_pack\":false}}'\nelse\n\
         echo '{{\"stale\":[\"{world}\"],\"needs_pack\":false}}'\nfi\nexit 0\nfi\n{build}\n",
        world = carriers::WORLD.name,
    );
    let compiler = carriers::kit_compiler(kit);
    fs::create_dir_all(compiler.parent().unwrap()).unwrap();
    fs::write(&compiler, script).unwrap();
    fs::set_permissions(compiler, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A build that adds each missing required carrier and stamps `generation`.
fn completing(generation: &str) -> String {
    let mut build = String::new();
    for carrier in carriers::required() {
        build.push_str(&format!(
            "[ -f \"$out/{0}\" ] || printf '{generation}' > \"$out/{0}\"\n",
            carrier.output
        ));
    }
    build.push_str(&format!("printf '{generation}' > \"$out/{STAMP_FILE}\""));
    build
}

/// Every required carrier and the stamp in `dir`, each holding `generation`.
fn write_set(dir: &Path, generation: &str) {
    fs::create_dir_all(dir).unwrap();
    for carrier in carriers::required() {
        fs::write(dir.join(carrier.output), generation).unwrap();
    }
    fs::write(dir.join(STAMP_FILE), generation).unwrap();
}

fn layout(data: &Dir) -> InstallLayout {
    let mut layout = installed_layout(data, "/opt/cinnabar/bin/cinnabar");
    layout.resource_root = data.path().join("resources");
    layout
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn a_finished_background_build_is_promoted_by_the_release_it_was_built_for() {
    let data = Dir::new("next-promote");
    let layout = layout(&data);
    let prepared = layout.prepared_assets_dir();
    write_set(&prepared, "v1");
    let offered = data.path().join("candidate/prep-kit");
    write_kit(&offered, "v2", &completing("v2"));

    build(&layout, &offered, &AtomicBool::new(false)).unwrap();
    let ready = next_dir(&layout).join(READY);
    assert_eq!(read(&ready.join(carriers::WORLD.output)), "v2");
    // Carriers the new kit still finds current carry over instead of rebuilding.
    let kept = carriers::required()
        .find(|carrier| carrier.name != carriers::WORLD.name)
        .unwrap();
    assert_eq!(read(&ready.join(kept.output)), "v1");
    assert_eq!(read(&prepared.join(carriers::WORLD.output)), "v1");
    assert!(!next_dir(&layout).join(KIT).exists());

    // The update installs the offered kit; its first launch only promotes.
    write_kit(&layout.prep_kit(), "v2", "exit 1");
    assert!(promote(&layout));
    assert_eq!(read(&prepared.join(carriers::WORLD.output)), "v2");
    assert!(!next_dir(&layout).exists());
}

#[test]
fn a_stale_or_partial_set_is_never_promoted() {
    let data = Dir::new("next-stale");
    let layout = layout(&data);
    let prepared = layout.prepared_assets_dir();
    write_set(&prepared, "v1");
    write_kit(&layout.prep_kit(), "v3", "exit 1");
    let ready = next_dir(&layout).join(READY);
    write_set(&ready, "v2");
    assert!(!promote(&layout), "a set for another release was promoted");

    write_set(&ready, "v3");
    let missing = carriers::required().next().unwrap();
    fs::remove_file(ready.join(missing.output)).unwrap();
    assert!(
        !promote(&layout),
        "a set missing a required carrier was promoted"
    );

    assert_eq!(read(&prepared.join(carriers::WORLD.output)), "v1");
    assert!(ready.is_dir(), "setup must still be able to resume from it");
}

#[test]
fn setup_resumes_an_interrupted_background_build() {
    let data = Dir::new("next-resume");
    let layout = layout(&data);
    write_set(&layout.prepared_assets_dir(), "v1");
    fs::create_dir_all(layout.vanilla_pack_dir().join("textures")).unwrap();
    write_kit(&layout.prep_kit(), "v2", &completing("v2"));
    // The background run finished one carrier before the client restarted.
    let building = next_dir(&layout).join(BUILDING);
    fs::create_dir_all(&building).unwrap();
    let finished = carriers::required().next().unwrap();
    fs::write(building.join(finished.output), "background").unwrap();

    prepare::prepare(&layout, &AtomicBool::new(false), &mut |_| {}).unwrap();

    let prepared = layout.prepared_assets_dir();
    assert_eq!(read(&prepared.join(finished.output)), "background");
    assert!(carriers::required_present(&prepared));
    assert!(!next_dir(&layout).exists());
}

#[test]
fn cancelling_a_background_build_leaves_nothing_to_promote() {
    let data = Dir::new("next-cancel");
    let layout = layout(&data);
    write_set(&layout.prepared_assets_dir(), "v1");
    let offered = data.path().join("candidate/prep-kit");
    write_kit(&offered, "v2", "touch \"$out/started\"\nexec sleep 30");
    let building = next_dir(&layout).join(BUILDING);
    let cancel = AtomicBool::new(false);
    let result = thread::scope(|scope| {
        let running = scope.spawn(|| build(&layout, &offered, &cancel));
        test_time::eventually("the background compiler to start", || {
            building.join("started").exists()
        });
        cancel.store(true, Ordering::Relaxed);
        running.join().unwrap()
    });
    assert!(result.unwrap_err().is::<Cancelled>());
    assert!(!next_dir(&layout).join(READY).exists());
    assert!(!promote(&layout));
    assert!(building.is_dir(), "the interrupted set stays resumable");
    assert_eq!(
        read(&layout.prepared_assets_dir().join(carriers::WORLD.output)),
        "v1"
    );
}
