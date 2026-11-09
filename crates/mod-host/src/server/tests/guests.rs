//! Real SDK example guests, built for wasm32 into one guest target per worktree. The library's
//! tests and the helper integration test both include this file, so every build into that target
//! takes the same lock.

/// Builds a real SDK example in an isolated guest target and wraps its Wasm as a component.
///
/// The build and the read hold an exclusive lock on the guest target across processes. Cargo
/// uplifts the requested artifact into `debug/` on every build, fresh or not, unless that file
/// is already a hard link to its build output: it removes the old file and then links or copies
/// the new one, and on macOS it always copies. Without the lock, a read that overlaps another
/// test's build can find no file or a partial one.
pub(super) fn component(package: &str) -> Vec<u8> {
    let exe = std::env::current_exe().unwrap();
    let target = exe
        .ancestors()
        .nth(3)
        .unwrap()
        .join(worktree_dir("mod-host-guests"));
    std::fs::create_dir_all(&target).unwrap();
    let lock = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(target.join("guests.lock"))
        .unwrap();
    lock.lock().unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new(cargo)
        .current_dir(&root)
        .args(["build", "--locked", "--target", "wasm32-unknown-unknown"])
        .args(["-p", package, "--target-dir"])
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "building {package} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let artifact = package.replace('-', "_");
    let module =
        std::fs::read(target.join(format!("wasm32-unknown-unknown/debug/{artifact}.wasm")))
            .unwrap();
    drop(lock);
    wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}

/// `name` keyed by this worktree. Cargo judges freshness by modification time alone and its
/// dep-info paths are relative to the workspace, so two worktrees building into one target would
/// silently reuse each other's guests; keying the guests' target directory by worktree keeps
/// each worktree's guests its own.
fn worktree_dir(name: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_MANIFEST_DIR").hash(&mut hasher);
    format!("{name}-{:016x}", hasher.finish())
}
