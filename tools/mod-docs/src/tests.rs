use super::*;
use std::collections::BTreeSet;

/// Resolves a small documented contract without relying on the checkout's WIT files.
fn fixture(body: &str) -> Resolve {
    let mut resolve = Resolve::default();
    resolve
        .push_str("fixture.wit", &format!("package test:extension;\n{body}"))
        .unwrap();
    resolve
}

#[test]
fn committed_book_is_current() {
    generate(&repository(), true).unwrap();
}

#[test]
fn readable_titles_keep_exact_wit_identifiers() {
    for (package, interface, title) in [
        ("extension", "presentation", "Presentation"),
        ("extension", "action-filter", "Action filter"),
        ("extension", "player-state", "Player state"),
        ("extension", "hud", "HUD"),
        ("world-draw", "assets", "World drawing: assets"),
        ("world-draw", "drawing", "World drawing: drawing"),
        ("world-draw", "masks", "World drawing: masks"),
        ("server-experience", "ui", "Server experience: UI"),
        ("session", "items", "Session: items"),
        (
            "future-tools",
            "new-interface",
            "Future tools: new interface",
        ),
    ] {
        let mut resolve = Resolve::default();
        resolve
            .push_str(
                "title.wit",
                &format!(
                    "package test:{package}@1.2.3;\n\
                     /// A documented interface.\n\
                     /// Grants: None.\n\
                     interface {interface} {{}}\n\
                     /// A component.\n\
                     world component {{ import {interface}; }}"
                ),
            )
            .unwrap();
        let pages = pages(&resolve).unwrap();
        let files = interfaces(&resolve).unwrap();
        let file = files.keys().next().unwrap();
        let page = &pages[&format!("reference/{file}.md")];
        assert!(
            page.starts_with(&format!(
                "{HEADER}# {title}\n\n`test:{package}/{interface}@1.2.3`\n\n"
            )),
            "{package}/{interface}: expected heading {title} and exact WIT identifier"
        );
        assert!(pages["SUMMARY.md"].contains(&format!("[{title}](reference/{file}.md)")));
        assert!(pages["reference/index.md"].contains(&format!("[{title}]({file}.md)")));
        assert!(
            pages["reference/worlds.md"]
                .contains(&format!("[test:{package}/{interface}@1.2.3]({file}.md)"))
        );
    }
}

#[test]
fn title_overrides_are_current_and_titles_are_distinct() {
    let resolve = resolve(&repository()).unwrap();
    assert!(
        resolve
            .packages
            .iter()
            .any(|(_, package)| package.name.name == PRIMARY_PACKAGE),
        "missing primary reference package: {PRIMARY_PACKAGE}"
    );
    let names: BTreeSet<_> = resolve
        .packages
        .iter()
        .map(|(_, package)| package.name.name.as_str())
        .chain(
            resolve
                .interfaces
                .iter()
                .filter_map(|(_, interface)| interface.name.as_deref()),
        )
        .collect();
    let mut overrides = BTreeSet::new();
    for (name, title) in render::TITLE_OVERRIDES {
        assert!(names.contains(name), "unused title override: {name}");
        assert!(overrides.insert(name), "duplicate title override: {name}");
        assert_ne!(*title, name.replace('-', " "), "redundant override: {name}");
    }
    let interfaces = interfaces(&resolve).unwrap();
    let renderer = render::Renderer {
        resolve: &resolve,
        interfaces: &interfaces,
    };
    let mut titles = BTreeSet::new();
    for id in interfaces.values() {
        let title = renderer.interface_title(*id);
        assert!(titles.insert(title.clone()), "duplicate title: {title}");
    }
}

#[test]
fn renders_members_signatures_aliases_and_cross_page_links() {
    let resolve = fixture(
        r#"
        /// Shared data.
        /// Grants: None.
        interface shared {
            /// Coordinates in blocks.
            record point {
                /// Vertical coordinate.
                y: f32,
            }
        }
        /// Operations.
        /// Grants: None.
        interface api {
            use shared.{point};
            /// The selection.
            variant selection {
                /// No selection.
                empty,
                /// One point.
                at(point),
            }
            /// Access modes.
            flags access {
                /// Allows reading.
                read,
            }
            /// Completion status.
            enum status {
                /// Work completed.
                done,
            }
            /// Tries a selection.
            select: func(value: option<list<point>>) -> result<selection, string>;
        }
        /// A component.
        world component {
            import api;
            /// Called after admission.
            export init: func();
        }
    "#,
    );
    let pages = pages(&resolve).unwrap();
    let api = &pages["reference/api.md"];
    assert!(api.contains("select: func(value: option<list<point>>) -> result<selection, string>;"));
    assert!(api.contains("[`point`](shared.md#type-point)"));
    for comment in [
        "No selection.",
        "One point.",
        "Allows reading.",
        "Work completed.",
    ] {
        assert!(api.contains(comment), "{comment}");
    }
    assert!(pages["reference/shared.md"].contains("Vertical coordinate."));
    assert!(pages["reference/worlds.md"].contains("Called after admission."));
}

#[test]
fn undocumented_functions_and_types_are_errors() {
    for member in [
        "run: func();",
        "record data { value: u32 }",
        "variant data { empty }",
        "enum data { empty }",
        "flags data { read }",
        "type data = list<u32>;",
    ] {
        let resolve = fixture(&format!(
            "/// An API.\n/// Grants: None.\ninterface api {{\n{member}\n}}"
        ));
        assert!(
            pages(&resolve)
                .unwrap_err()
                .to_string()
                .contains("missing WIT documentation"),
            "{member}"
        );
    }
    let resolve = fixture("/// A component.\nworld component { export init: func(); }");
    assert!(
        pages(&resolve)
            .unwrap_err()
            .to_string()
            .contains("missing WIT documentation")
    );
}

#[test]
fn undocumented_grants_are_errors() {
    let resolve = fixture("/// An API.\ninterface api {}");
    assert!(
        pages(&resolve)
            .unwrap_err()
            .to_string()
            .contains("missing Grants:")
    );
}

#[test]
fn freshness_detects_missing_edited_and_removed_pages() {
    let directory = tempfile::tempdir().unwrap();
    let pages = BTreeMap::from([("reference/api.md".into(), format!("{HEADER}# API\n"))]);
    assert!(sync(directory.path(), &pages, true).is_err());
    sync(directory.path(), &pages, false).unwrap();
    sync(directory.path(), &pages, true).unwrap();
    fs::write(directory.path().join("reference/api.md"), "changed").unwrap();
    assert!(sync(directory.path(), &pages, true).is_err());
    sync(directory.path(), &pages, false).unwrap();
    fs::write(
        directory.path().join("reference/old.md"),
        format!("{HEADER}old"),
    )
    .unwrap();
    assert!(sync(directory.path(), &pages, true).is_err());
    sync(directory.path(), &pages, false).unwrap();
    assert!(!directory.path().join("reference/old.md").exists());
}

/// Pages checked out with CRLF line endings still count as current.
#[test]
fn freshness_accepts_crlf_checkouts() {
    let directory = tempfile::tempdir().unwrap();
    let pages = BTreeMap::from([("reference/api.md".into(), format!("{HEADER}# API\n"))]);
    fs::create_dir_all(directory.path().join("reference")).unwrap();
    fs::write(
        directory.path().join("reference/api.md"),
        format!("{HEADER}# API\n").replace('\n', "\r\n"),
    )
    .unwrap();
    sync(directory.path(), &pages, true).unwrap();
}
