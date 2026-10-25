use super::dependency_fixture::{diagnostics, fixture, set_dependencies, write};

#[test]
fn ui_world_boundary_accepts_lower_reads_and_streaming_test_fixtures() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-ui",
        "[dependencies]\nclient-world={path='../client-world'}\n[dev-dependencies]\nfixtures={package='chunk-pipeline',path='../chunk-pipeline'}",
    );
    write(
        &temp.path().join("crates/client-ui/src/adapter.rs"),
        "fn capture(_: client_world::ui_read::InventoryUi<'_>, _: client_world::ui_read::LocalPlayerUi<'_>) {}\n#[cfg(test)] mod tests { use fixtures::WorldStream; }",
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}

#[test]
fn ui_world_boundary_rejects_direct_renamed_and_target_specific_streaming_edges() {
    for kind in [
        "dependencies",
        "build-dependencies",
        "target.'cfg(windows)'.dependencies",
        "target.'cfg(unix)'.build-dependencies",
    ] {
        for declaration in [
            "chunk-pipeline={path='../chunk-pipeline'}",
            "renamed={package='chunk-pipeline',path='../chunk-pipeline'}",
            "renamed={package='chunk-pipeline',version='1'}",
        ] {
            let temp = fixture();
            set_dependencies(
                temp.path(),
                "client-ui",
                &format!("[{kind}]\n{declaration}"),
            );
            assert!(
                diagnostics(temp.path()).iter().any(|line| line.contains(
                    "client-ui: forbidden dependency path `client-ui -> chunk-pipeline`"
                )),
                "{kind}: {declaration}"
            );
        }
    }
}

#[test]
fn ui_world_boundary_rejects_workspace_inherited_streaming_aliases() {
    let temp = fixture();
    let manifest = temp.path().join("Cargo.toml");
    let mut source = std::fs::read_to_string(&manifest).expect("workspace fixture");
    source.push_str("[workspace.dependencies]\nstreaming={package='chunk-pipeline',path='crates/chunk-pipeline'}\n");
    write(&manifest, &source);
    set_dependencies(
        temp.path(),
        "client-ui",
        "[target.'cfg(windows)'.dependencies]\nstreaming.workspace=true",
    );
    assert!(diagnostics(temp.path()).iter().any(|line| {
        line.contains("client-ui: forbidden dependency path `client-ui -> chunk-pipeline`")
    }));
}

#[test]
fn ui_world_boundary_rejects_streaming_through_a_lower_production_dependency() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-ui",
        "[dependencies]\nui={path='../ui'}",
    );
    set_dependencies(
        temp.path(),
        "ui",
        "[target.'cfg(windows)'.dependencies]\nstreaming={package='chunk-pipeline',path='../chunk-pipeline'}",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("client-ui -> ui -> chunk-pipeline"))
    );
}

#[test]
fn ui_world_boundary_does_not_inherit_a_dependency_test_fixture() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-ui",
        "[dependencies]\nui={path='../ui'}",
    );
    set_dependencies(
        temp.path(),
        "ui",
        "[dev-dependencies]\nchunk-pipeline={path='../chunk-pipeline'}",
    );
    assert!(
        !diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("client-ui: forbidden dependency path"))
    );
}

#[test]
fn ui_world_boundary_keeps_production_test_support_protection() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-ui",
        "[features]\ntest-support=[]\nalias=['test-support']\n[dev-dependencies]\nchunk-pipeline={path='../chunk-pipeline'}",
    );
    write(
        &temp.path().join("app/Cargo.toml"),
        "[package]\nname='cinnabar'\nversion='0.1.0'\n[dependencies]\nclient-ui={path='../crates/client-ui',features=['alias']}",
    );
    assert!(diagnostics(temp.path()).iter().any(|line| line.contains(
        "production dependency `client-ui` enables test-support feature `test-support`"
    )));
}
