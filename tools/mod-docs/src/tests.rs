use super::*;

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
