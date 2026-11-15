use super::*;
use crate::LayeredPackView;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Isolated library storage plus a development folder, removed on drop.
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cinnabar-development-pack-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn development(&self) -> PathBuf {
        self.0.join("development_resource_packs")
    }

    /// Opens the library over this fixture's storage and development folder.
    fn library(&self) -> GlobalPackLibrary {
        let mut library = GlobalPackLibrary::open(self.0.join("installed"), [9, 0, 0]).unwrap();
        library.set_development_root(self.development());
        library
    }

    /// Writes `files` below the development folder `folder`.
    fn pack(&self, folder: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let root = self.development().join(folder);
        for (name, bytes) in files {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Original fixture manifest with one module of `kind`.
fn manifest(id: u128, kind: &str) -> String {
    let id = Uuid::from_u128(id);
    format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","name":"Fixture","version":[1,0,0]}},"modules":[{{"type":"{kind}"}}]}}"#
    )
}

/// Reads `marker.txt` from the library's current staged selection.
fn marker(library: &mut GlobalPackLibrary) -> Vec<u8> {
    let view = LayeredPackView::new(library.preview().unwrap());
    view.read("marker.txt").unwrap().to_vec()
}

#[test]
fn folder_edits_apply_without_reimport_or_version_change() {
    let fixture = Fixture::new();
    let folder = fixture.pack(
        "my_pack",
        &[
            ("manifest.json", manifest(1, "resources").as_bytes()),
            ("marker.txt", b"first"),
        ],
    );
    let mut library = fixture.library();
    let listed: Vec<_> = library.installed().map(|pack| pack.id).collect();
    assert_eq!(listed, [Uuid::from_u128(1)]);
    assert!(library.available().is_empty(), "nothing is copied");
    library.activate(Uuid::from_u128(1)).unwrap();
    library.apply().unwrap();
    assert_eq!(marker(&mut library), b"first");

    fs::write(folder.join("marker.txt"), b"second").unwrap();
    assert_eq!(marker(&mut library), b"second");

    let mut restarted = fixture.library();
    assert!(restarted.is_development(Uuid::from_u128(1)));
    assert_eq!(marker(&mut restarted), b"second");
}

#[test]
fn an_unchanged_folder_previews_as_the_same_contents() {
    let fixture = Fixture::new();
    let folder = fixture.pack(
        "my_pack",
        &[
            ("manifest.json", manifest(1, "resources").as_bytes()),
            ("marker.txt", b"same"),
        ],
    );
    let mut library = fixture.library();
    library.activate(Uuid::from_u128(1)).unwrap();
    let first = library.preview().unwrap();
    assert!(first.same_contents(&library.preview().unwrap()));
    fs::write(folder.join("marker.txt"), b"edited").unwrap();
    assert!(!first.same_contents(&library.preview().unwrap()));
}

#[test]
fn a_removed_folder_leaves_the_selection_and_the_rest_still_apply() {
    let fixture = Fixture::new();
    let gone = fixture.pack(
        "gone",
        &[
            ("manifest.json", manifest(1, "resources").as_bytes()),
            ("marker.txt", b"gone"),
        ],
    );
    fixture.pack(
        "kept",
        &[
            ("manifest.json", manifest(2, "resources").as_bytes()),
            ("marker.txt", b"kept"),
        ],
    );
    let mut library = fixture.library();
    library.activate(Uuid::from_u128(2)).unwrap();
    library.activate(Uuid::from_u128(1)).unwrap();
    library.apply().unwrap();
    fs::remove_dir_all(gone).unwrap();

    let mut restarted = fixture.library();
    assert_eq!(
        restarted.missing_packs(),
        [format!("development pack {}", Uuid::from_u128(1))]
    );
    assert_eq!(marker(&mut restarted), b"kept");

    assert!(library.scan_development());
    assert_eq!(library.active().len(), 1);
    assert_eq!(marker(&mut library), b"kept");
}

#[test]
fn a_development_pack_shadows_an_imported_pack_with_its_uuid() {
    let fixture = Fixture::new();
    let imported = fixture.0.join("imported");
    for (name, bytes) in [
        ("manifest.json", manifest(1, "resources").into_bytes()),
        ("marker.txt", b"imported".to_vec()),
    ] {
        fs::create_dir_all(&imported).unwrap();
        fs::write(imported.join(name), bytes).unwrap();
    }
    let mut library = fixture.library();
    library.import(&imported).unwrap();
    library.activate(Uuid::from_u128(1)).unwrap();
    assert_eq!(marker(&mut library), b"imported");

    fixture.pack(
        "working_copy",
        &[
            ("manifest.json", manifest(1, "resources").as_bytes()),
            ("marker.txt", b"development"),
        ],
    );
    assert!(library.scan_development());
    assert_eq!(library.installed().count(), 1);
    assert_eq!(marker(&mut library), b"development");
}

#[test]
fn scans_report_unusable_folders_and_skip_behavior_packs() {
    let fixture = Fixture::new();
    let resources = manifest(1, "resources");
    let behavior = manifest(2, "data");
    fixture.pack("a_first", &[("manifest.json", resources.as_bytes())]);
    fixture.pack("b_repeat", &[("manifest.json", resources.as_bytes())]);
    fixture.pack("c_behavior", &[("manifest.json", behavior.as_bytes())]);
    fixture.pack("d_empty", &[("readme.txt", b"no manifest")]);
    let library = fixture.library();
    let listed: Vec<_> = library.installed().map(|pack| pack.id).collect();
    assert_eq!(listed, [Uuid::from_u128(1)]);
    let rejected = library.development_rejections();
    assert_eq!(rejected.len(), 2, "{rejected:?}");
    assert!(rejected[0].starts_with("b_repeat: "), "{rejected:?}");
    assert!(rejected[1].starts_with("d_empty: "), "{rejected:?}");
}

#[test]
fn icons_and_display_names_come_from_the_folder() {
    let fixture = Fixture::new();
    let manifest = manifest(1, "resources");
    let manifest = manifest.replace("Fixture", "pack.name");
    fixture.pack(
        "my_pack",
        &[
            ("manifest.json", manifest.as_bytes()),
            ("pack_icon.png", b"icon"),
            ("texts/en_US.lang", b"pack.name=Working copy"),
        ],
    );
    let library = fixture.library();
    let pack = library.installed().next().unwrap().clone();
    assert_eq!(pack.name, "Working copy");
    let icon = library.pack_icon(&pack).unwrap().unwrap();
    assert_eq!(icon.as_ref(), b"icon");
}
