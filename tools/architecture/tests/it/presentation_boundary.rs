use super::dependency_fixture::{diagnostics, fixture, set_dependencies, write};

#[test]
fn presentation_accepts_borrowed_observations_and_shared_asset_contracts() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-presentation",
        "[dependencies]\nassets={path='../assets'}\nclient-world={package='chunk-pipeline',path='../chunk-pipeline'}\nrender-model={path='../render-model'}\nrender-scene={path='../render-scene'}",
    );
    write(
        &temp
            .path()
            .join("crates/client-presentation/src/adapter.rs"),
        "struct Observation<'a> { stream: Option<&'a mut client_world::WorldStream>, state: &'a sim::PlayerState }\nfn observe(_: render_scene::ActorRenderScene, _: &render_model::ActorRigGeometry) {}",
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}

#[test]
fn plugins_reject_host_gameplay_session_and_reverse_evidence_edges() {
    for owner in ["client-presentation", "acceptance", "diagnostics"] {
        for package in ["cinnabar", "gameplay", "client-session"] {
            for kind in [
                "dependencies",
                "dev-dependencies",
                "target.'cfg(windows)'.build-dependencies",
            ] {
                let temp = fixture();
                set_dependencies(
                    temp.path(),
                    owner,
                    &format!("[{kind}]\nalias={{package='{package}',version='1'}}"),
                );
                assert!(
                    diagnostics(temp.path())
                        .iter()
                        .any(|line| line.contains("forbidden dependency path")),
                    "{owner} -> {package}"
                );
            }
        }
    }
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-presentation",
        "[dependencies]\nacceptance={path='../acceptance'}",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("client-presentation -> acceptance"))
    );
}

#[test]
fn plugins_reject_app_modules_and_authority_ownership() {
    for owner in ["client-presentation", "acceptance"] {
        for source in [
            "use crate::{runtime as host}; fn read(_: host::World) {}",
            "use crate::movement::State;",
            "struct Owner { stream: client_world::WorldStream }",
            "struct Owner { player: player_state::PlayerState }",
            "struct Owner { network: NetworkHandle }",
        ] {
            let temp = fixture();
            write(
                &temp.path().join(format!("crates/{owner}/src/adapter.rs")),
                source,
            );
            assert!(
                diagnostics(temp.path())
                    .iter()
                    .any(|line| line.contains("forbidden")),
                "missed {owner}: {source}"
            );
        }
    }
}

#[test]
fn presentation_cannot_hide_session_dependency_behind_shared_crate() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-presentation",
        "[dependencies]\nclient-ui={path='../client-ui'}",
    );
    set_dependencies(
        temp.path(),
        "client-ui",
        "[dependencies]\ntransport={package='client-session',version='1'}",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("client-presentation -> client-ui -> client-session"))
    );
}

#[test]
fn presentation_test_support_cannot_leak_into_production() {
    let temp = fixture();
    write(
        &temp.path().join("app/Cargo.toml"),
        "[package]\nname='cinnabar'\nversion='0.1.0'\n[dependencies]\nclient-presentation={path='../crates/client-presentation',features=['test-support']}",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("test-support"))
    );
}

#[test]
fn environment_derivation_rejects_app_world_ui_and_preference_wrappers() {
    for source in [
        "fn derive(_: ClientWorld) {}",
        "fn derive(_: UiRuntimeResource) {}",
        "fn derive(_: BrowserOptions) {}",
        "fn derive(_: client_shell::menu::MenuRuntime) {}",
    ] {
        let temp = fixture();
        write(
            &temp
                .path()
                .join("crates/client-presentation/src/environment.rs"),
            source,
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line.contains("crosses forbidden")),
            "{source}"
        );
    }
}
