use std::{fs, path::Path};

use architecture::check_repository;

/// Writes one fixture source after creating its parent directories.
fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// Creates an extracted crate that owns one symbolic environment marker.
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers=['crates/screens']\n",
    );
    write(
        root,
        "crates/screens/Cargo.toml",
        "[package]\nname='screens'\nversion='0.1.0'\n",
    );
    write(
        root,
        "policy.toml",
        r#"
production_rust_max = 1000
module_root_max = 300
powershell_max = 800
test_max = 1200
[[crates]]
name = "screens"
path = "crates/screens"
[[markers]]
literal = "CINNABAR_READY"
kind = "environment_variable"
producer = "crates/screens/src/diagnostic_markers.rs"
consumer = "crates/screens/src/observer.rs"
"#,
    );
    write(
        root,
        "crates/screens/src/diagnostic_markers.rs",
        "pub const READY: &str = \"CINNABAR_READY\";\n",
    );
    write(
        root,
        "crates/screens/src/observer.rs",
        "fn observe() { let _ = std::env::var(crate::diagnostic_markers::READY); }\n",
    );
    temp
}

#[test]
fn extracted_owner_preserves_the_symbolic_marker_contract() {
    let temp = fixture();
    assert_eq!(
        check_repository(temp.path(), &temp.path().join("policy.toml")).unwrap(),
        Vec::<String>::new()
    );
}

/// The constant's owning crate may expose it directly rather than through a markers module.
#[test]
fn extracted_owner_accepts_a_qualified_contract_constant() {
    let temp = fixture();
    write(
        temp.path(),
        "crates/screens/src/observer.rs",
        "fn observe() { let _ = std::env::var(render_api::READY); }\n",
    );
    assert_eq!(
        check_repository(temp.path(), &temp.path().join("policy.toml")).unwrap(),
        Vec::<String>::new()
    );
}

/// A Rust parser consumes the same canonical constant instead of repeating its wire text.
#[test]
fn extracted_owner_accepts_a_parsed_symbolic_contract() {
    let temp = fixture();
    let policy = temp.path().join("policy.toml");
    let source = fs::read_to_string(&policy).unwrap();
    fs::write(&policy, source.replace("environment_variable", "parsed")).unwrap();
    write(
        temp.path(),
        "crates/screens/src/observer.rs",
        "fn observe(line: &str) { let _ = line.split_once(render_api::READY); }\n",
    );
    assert_eq!(
        check_repository(temp.path(), &policy).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn extracted_owner_rejects_undeclared_markers_in_its_registry() {
    let temp = fixture();
    write(
        temp.path(),
        "crates/screens/src/diagnostic_markers.rs",
        "pub const READY: &str = \"CINNABAR_READY\";\npub const OTHER: &str = \"CINNABAR_UNDECLARED\";\n",
    );
    let diagnostics = check_repository(temp.path(), &temp.path().join("policy.toml")).unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|line| line == "marker `CINNABAR_UNDECLARED` has no declared expectation")
    );
}

/// Ordinary environment variables share the marker prefix outside the registry.
#[test]
fn extracted_owner_permits_environment_variables_outside_its_registry() {
    let temp = fixture();
    write(
        temp.path(),
        "crates/screens/src/other.rs",
        "fn title() { let _ = std::env::var(\"CINNABAR_WINDOW_TITLE\"); }\n",
    );
    assert_eq!(
        check_repository(temp.path(), &temp.path().join("policy.toml")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn extracted_owner_requires_the_declared_consumer_to_use_the_symbol() {
    let temp = fixture();
    for source in [
        "fn observe() {}\n",
        "fn observe() { let _ = render_api::READY_EXTRA; }\n",
        "fn observe() { let _ = \"render_api::READY\"; }\n",
        "// render_api::READY\nfn observe() {}\n",
    ] {
        write(temp.path(), "crates/screens/src/observer.rs", source);
        let diagnostics = check_repository(temp.path(), &temp.path().join("policy.toml")).unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|line| line.contains("has no symbolic use in declared consumer")),
            "non-use satisfied the marker contract: {source}"
        );
    }
}
