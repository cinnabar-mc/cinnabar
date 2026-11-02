/// Builds a real SDK example in an isolated guest target and wraps its Wasm as a component.
pub(super) fn component(package: &str) -> Vec<u8> {
    let exe = std::env::current_exe().unwrap();
    let target = exe
        .ancestors()
        .nth(3)
        .unwrap()
        .join(super::worktree_dir("mod-host-guests"));
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
    wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}
