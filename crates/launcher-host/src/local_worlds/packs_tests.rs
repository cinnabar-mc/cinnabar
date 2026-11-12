use std::{fs, path::Path};

use launcher::local_worlds::packs::PackKind;
use serde_json::{Value, json};

use super::*;
use crate::local_worlds::{
    store::Store,
    test_support::{build_zip, scratch},
};

const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";

fn manifest(id: &str, version: [u32; 3], module: &str, name: &str) -> String {
    json!({
        "format_version": 2,
        "header": { "name": name, "description": "d", "uuid": id, "version": version },
        "modules": [{ "type": module, "uuid": B, "version": [1, 0, 0] }],
    })
    .to_string()
}

struct Fixture {
    dir: tempfile::TempDir,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let dir = scratch();
        let store = Store::open(dir.path().join("worlds")).expect("store");
        Self { dir, store }
    }

    fn level(&self) -> std::path::PathBuf {
        let level = self.dir.path().join("level");
        fs::create_dir_all(&level).unwrap();
        level
    }

    /// Adds the archive made of `files` to the level folder's list of `kind`.
    fn add(&self, kind: PackKind, files: &[(&str, &[u8])]) -> Result<(), WorldError> {
        let source = self.dir.path().join("pick.mcpack");
        fs::write(&source, build_zip(files)).unwrap();
        let staging = self.store.staging().expect("staging");
        add(&self.level(), kind, &source, &staging, &|| false)
    }

    fn ids(&self, kind: PackKind) -> Vec<String> {
        list(&self.level(), kind)
            .unwrap()
            .into_iter()
            .map(|pack| pack.id)
            .collect()
    }
}

fn entries(level: &Path, file: &str) -> Vec<Value> {
    serde_json::from_slice(&fs::read(level.join(file)).unwrap()).unwrap()
}

#[test]
fn a_resource_pack_installs_into_the_world_and_leads_its_list() {
    let fixture = Fixture::new();
    let first = manifest(A, [1, 0, 2], "resources", "First");
    fixture
        .add(PackKind::Resource, &[("manifest.json", first.as_bytes())])
        .unwrap();
    let second = manifest(B, [0, 1, 0], "resources", "Second");
    fixture
        .add(
            PackKind::Resource,
            &[("Wrapped/manifest.json", second.as_bytes())],
        )
        .unwrap();

    let level = fixture.level();
    assert!(
        level
            .join("resource_packs")
            .join(A)
            .join("manifest.json")
            .is_file()
    );
    assert!(
        level
            .join("resource_packs")
            .join(B)
            .join("manifest.json")
            .is_file()
    );
    assert_eq!(
        entries(&level, "world_resource_packs.json"),
        vec![
            json!({ "pack_id": B, "version": [0, 1, 0] }),
            json!({ "pack_id": A, "version": [1, 0, 2] }),
        ]
    );
    let listed = list(&level, PackKind::Resource).unwrap();
    assert_eq!(listed[0].name, "Second");
    assert_eq!(listed[1].version_text(), "1.0.2");
    assert!(listed.iter().all(|pack| !pack.external));
    assert!(fixture.ids(PackKind::Behaviour).is_empty());
}

#[test]
fn a_bundle_adds_only_the_requested_kind_and_names_the_other_tab() {
    let fixture = Fixture::new();
    let resources = manifest(A, [1, 0, 0], "resources", "Art");
    let behaviour = manifest(B, [1, 0, 0], "data", "Rules");
    let bundle: &[(&str, &[u8])] = &[
        ("art/manifest.json", resources.as_bytes()),
        ("rules/manifest.json", behaviour.as_bytes()),
    ];
    fixture.add(PackKind::Behaviour, bundle).unwrap();
    assert_eq!(fixture.ids(PackKind::Behaviour), [B]);
    assert!(fixture.ids(PackKind::Resource).is_empty());
    assert!(!fixture.level().join("resource_packs").exists());

    let only_resources: &[(&str, &[u8])] = &[("manifest.json", resources.as_bytes())];
    let Err(WorldError::Rejected(message)) = fixture.add(PackKind::Behaviour, only_resources)
    else {
        panic!("a resource pack is not a behaviour pack");
    };
    assert!(message.contains("resource pack"), "{message}");
    assert_eq!(fixture.ids(PackKind::Behaviour), [B]);
}

#[test]
fn adding_a_newer_version_replaces_the_old_copy() {
    let fixture = Fixture::new();
    for version in [[1, 0, 0], [2, 0, 0]] {
        let text = manifest(A, version, "data", "Rules");
        fixture
            .add(PackKind::Behaviour, &[("manifest.json", text.as_bytes())])
            .unwrap();
    }
    let level = fixture.level();
    assert_eq!(
        entries(&level, "world_behavior_packs.json"),
        vec![json!({ "pack_id": A, "version": [2, 0, 0] })]
    );
    let held = fs::read_dir(level.join("behavior_packs")).unwrap().count();
    assert_eq!(held, 1);
    assert_eq!(
        list(&level, PackKind::Behaviour).unwrap()[0].version,
        [2, 0, 0]
    );
}

#[test]
fn moving_and_removing_keep_every_other_entry_intact() {
    let fixture = Fixture::new();
    let level = fixture.level();
    let a = manifest(A, [1, 0, 0], "resources", "A");
    fixture
        .add(PackKind::Resource, &[("manifest.json", a.as_bytes())])
        .unwrap();
    // A hand-written entry the launcher does not own keeps its extra fields.
    let mut written = entries(&level, "world_resource_packs.json");
    written.push(json!({ "pack_id": B, "version": [3, 0, 0], "subpack": "high" }));
    fs::write(
        level.join("world_resource_packs.json"),
        serde_json::to_vec(&written).unwrap(),
    )
    .unwrap();

    move_to(&level, PackKind::Resource, A, 1).unwrap();
    assert_eq!(fixture.ids(PackKind::Resource), [B, A]);
    move_to(&level, PackKind::Resource, A, 99).unwrap();
    assert_eq!(fixture.ids(PackKind::Resource), [B, A]);
    move_to(&level, PackKind::Resource, A, 0).unwrap();
    assert_eq!(fixture.ids(PackKind::Resource), [A, B]);
    assert!(matches!(
        move_to(&level, PackKind::Resource, "missing", 0),
        Err(WorldError::NotFound)
    ));

    remove(&level, PackKind::Resource, A).unwrap();
    assert!(!level.join("resource_packs").join(A).exists());
    assert_eq!(
        entries(&level, "world_resource_packs.json"),
        vec![json!({ "pack_id": B, "version": [3, 0, 0], "subpack": "high" })]
    );
    let left = list(&level, PackKind::Resource).unwrap();
    assert!(
        left[0].external,
        "a pack without a copy is listed as external"
    );
    assert_eq!(left[0].name, B);
}

#[test]
fn a_refused_file_changes_nothing() {
    let fixture = Fixture::new();
    let level = fixture.level();
    fs::write(level.join("world_behavior_packs.json"), b"not json").unwrap();
    let text = manifest(A, [1, 0, 0], "data", "Rules");
    let good: &[(&str, &[u8])] = &[("manifest.json", text.as_bytes())];
    assert!(matches!(
        fixture.add(PackKind::Behaviour, good),
        Err(WorldError::Rejected(_))
    ));
    assert_eq!(
        fs::read(level.join("world_behavior_packs.json")).unwrap(),
        b"not json"
    );
    assert!(!level.join("behavior_packs").join(A).exists());

    fs::remove_file(level.join("world_behavior_packs.json")).unwrap();
    let readme: &[(&str, &[u8])] = &[("readme.txt", b"hello")];
    let broken: &[(&str, &[u8])] = &[("manifest.json", b"{")];
    let escaping: &[(&str, &[u8])] = &[("../manifest.json", text.as_bytes())];
    for files in [readme, broken, escaping] {
        assert!(
            matches!(
                fixture.add(PackKind::Behaviour, files),
                Err(WorldError::Rejected(_))
            ),
            "{files:?}"
        );
    }
    assert!(fixture.ids(PackKind::Behaviour).is_empty());

    let source = fixture.dir.path().join("pick.txt");
    fs::write(&source, build_zip(good)).unwrap();
    let staging = fixture.store.staging().unwrap();
    assert!(matches!(
        add(&level, PackKind::Behaviour, &source, &staging, &|| false),
        Err(WorldError::Rejected(_))
    ));
}

/// A Bedrock game folder holding `manifest` as the development pack `name` of `folder`.
fn game_pack(root: &Path, folder: &str, name: &str, manifest: &str) {
    let dir = root.join(folder).join(name);
    fs::create_dir_all(dir.join("scripts")).unwrap();
    fs::write(dir.join("manifest.json"), manifest).unwrap();
    fs::write(dir.join("scripts/main.js"), name).unwrap();
}

#[test]
fn a_listed_pack_is_found_in_the_game_folders_and_copied_into_the_world() {
    let fixture = Fixture::new();
    let level = fixture.level();
    // The world lists the pack at an older version, plus another entry that must stay as it is.
    fs::write(
        level.join("world_behavior_packs.json"),
        json!([
            { "pack_id": B, "version": [3, 0, 0], "subpack": "high" },
            { "pack_id": A, "version": [1, 0, 2] },
        ])
        .to_string(),
    )
    .unwrap();
    let game = fixture.dir.path().join("game");
    let older = manifest(A, [1, 0, 1], "script", "Older");
    let newest = manifest(A, [1, 0, 3], "script", "Newest");
    game_pack(&game, "behavior_packs", "Older", &older);
    game_pack(&game, "development_behavior_packs", "Newest", &newest);
    assert!(list(&level, PackKind::Behaviour).unwrap()[1].external);

    let staging = fixture.store.staging().unwrap();
    find(&level, PackKind::Behaviour, A, &[game.clone()], &staging).unwrap();

    assert_eq!(
        entries(&level, "world_behavior_packs.json"),
        vec![
            json!({ "pack_id": B, "version": [3, 0, 0], "subpack": "high" }),
            json!({ "pack_id": A, "version": [1, 0, 3] }),
        ],
        "the entry keeps its place and follows the copy's version"
    );
    assert_eq!(
        fs::read_to_string(level.join("behavior_packs").join(A).join("scripts/main.js")).unwrap(),
        "Newest",
        "with no exact version the newest copy wins"
    );
    let listed = list(&level, PackKind::Behaviour).unwrap();
    assert_eq!(
        (listed[1].name.as_str(), listed[1].external),
        ("Newest", false)
    );
    assert!(listed[0].external, "other entries are left alone");
}

#[test]
fn the_copy_matching_the_listed_version_wins_and_a_missing_one_changes_nothing() {
    let fixture = Fixture::new();
    let level = fixture.level();
    fs::write(
        level.join("world_resource_packs.json"),
        json!([{ "pack_id": A, "version": [1, 0, 1] }]).to_string(),
    )
    .unwrap();
    let game = fixture.dir.path().join("game");
    game_pack(
        &game,
        "resource_packs",
        "Exact",
        &manifest(A, [1, 0, 1], "resources", "Exact"),
    );
    game_pack(
        &game,
        "resource_packs",
        "Newer",
        &manifest(A, [2, 0, 0], "resources", "Newer"),
    );
    let staging = fixture.store.staging().unwrap();
    find(&level, PackKind::Resource, A, &[game], &staging).unwrap();
    assert_eq!(list(&level, PackKind::Resource).unwrap()[0].name, "Exact");

    // A listed pack no game folder holds is refused and the world is left as it was.
    let lists = json!([{ "pack_id": B, "version": [9, 9, 9] }]).to_string();
    fs::write(level.join("world_resource_packs.json"), &lists).unwrap();
    let Err(WorldError::Rejected(message)) = find(
        &level,
        PackKind::Resource,
        B,
        &[fixture.dir.path().join("none")],
        &staging,
    ) else {
        panic!("no folder holds the pack");
    };
    assert!(message.contains("Minecraft"), "{message}");
    assert_eq!(
        fs::read_to_string(level.join("world_resource_packs.json")).unwrap(),
        lists
    );
    assert!(!level.join("resource_packs").join(B).exists());
    // An entry the world does not list is not looked for.
    assert!(matches!(
        find(&level, PackKind::Resource, A, &[], &staging),
        Err(WorldError::NotFound)
    ));
}
