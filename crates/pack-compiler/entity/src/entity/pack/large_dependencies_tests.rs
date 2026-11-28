use super::*;
use serde_json::json;

/// Builds a small renderable entity with an independently sized animation alias dictionary.
fn entity_files(alias_count: usize, unique: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let aliases = (0..alias_count)
        .map(|index| {
            let target = if unique { index } else { 0 };
            (
                format!("clip_{index}"),
                json!(format!("animation.fixture_{target}")),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let clips = (0..if unique { alias_count } else { 1 })
        .map(|index| {
            (
                format!("animation.fixture_{index}"),
                json!({
                    "loop":true,"bones":{"root":{"rotation":[0,10,0]}}
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    [
        ("entity/fixture.json", json!({
            "format_version":"1.10.0", "minecraft:client_entity":{"description":{
                "identifier":"fixture:large_entity", "geometry":{"default":"geometry.fixture"},
                "animations":aliases, "scripts":{"animate":["clip_0"]},
                "render_controllers":["controller.render.fixture"]
            }}
        })),
        ("models/entity/fixture.json", json!({
            "format_version":"1.12.0", "minecraft:geometry":[{
                "description":{"identifier":"geometry.fixture","texture_width":16,"texture_height":16},
                "bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]
            }]
        })),
        ("animations/fixture.json", json!({"format_version":"1.8.0","animations":clips})),
        ("render_controllers/fixture.json", json!({
            "format_version":"1.8.0", "render_controllers":{
                "controller.render.fixture":{"geometry":"Geometry.default"}
            }
        })),
    ].into_iter().map(|(path,value)| (path.into(),serde_json::to_vec(&value).unwrap())).collect()
}

/// Checks both pack admission and the encoded carrier consumed by the client.
fn assert_retained(alias_count: usize, unique: bool, malformed_neighbor: bool) {
    let mut files = entity_files(alias_count, unique);
    if malformed_neighbor {
        files.push(("entity/bad.json".into(), b"{invalid".to_vec()));
    }
    let compilation = compile_entity_pack(files)
        .unwrap()
        .expect("valid entity must remain admitted");
    assert_eq!(
        compilation.skipped.unparsable,
        u32::from(malformed_neighbor)
    );
    let entity = compilation
        .assets
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == EntityAssetKind::Entity
                && symbol.identifier.as_ref() == "fixture:large_entity"
        })
        .expect("entity symbol must survive alias collection");
    let animation_count = entity
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind == EntityDependencyKind::Animation)
        .count();
    assert_eq!(animation_count, if unique { alias_count } else { 1 });
    assert!(
        !compilation.assets.rig_bindings.is_empty(),
        "the entity must retain a render rig"
    );
    let bytes = assets::encode_entity_blob(&compilation.assets).unwrap();
    let runtime = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
    assert!(
        runtime
            .symbols()
            .iter()
            .any(|symbol| symbol.identifier.as_ref() == "fixture:large_entity")
    );
    assert!(!runtime.rig_bindings().is_empty());
}

#[test]
fn large_dependencies_repeated_animation_aliases_preserve_the_entity() {
    assert_retained(700, false, false);
}

#[test]
fn large_dependencies_unique_references_round_trip_with_the_rig() {
    assert_retained(900, true, false);
}

#[test]
fn large_dependencies_bad_neighbor_does_not_remove_a_valid_entity() {
    assert_retained(700, false, true);
}

/// Synthetic identifiers cross the real 512 MiB catalog budget without a pack fixture:
/// the first source that exceeds it is the one reported, and later sources are never walked.
#[test]
fn entity_dependency_budget_rejects_at_512mib_before_all_sources_accumulate() {
    use assets::{
        EntityAssetKind, EntityDependency, EntityDependencyKind, EntityDependencyResolution,
        EntityDependencyStorageBudget, MAX_ENTITY_CATALOG_BYTES,
    };
    use std::collections::BTreeMap;

    use super::super::{PendingSymbol, charge_retained_dependencies};

    const CHUNK: usize = 128 * 1024 * 1024;
    let identifier: Box<str> = "x".repeat(CHUNK).into();
    let dependency = || EntityDependency {
        kind: EntityDependencyKind::Animation,
        identifier: identifier.clone(),
        resolution: EntityDependencyResolution::Catalog,
    };
    let symbol = |name: &str, count: usize| PendingSymbol {
        kind: EntityAssetKind::Entity,
        identifier: name.into(),
        source_path: format!("entity/{name}.json").into(),
        dependencies: (0..count).map(|_| dependency()).collect(),
        charged_dependencies: 0,
    };

    // Control: the same storage estimate below the real bound is fully charged once.
    let mut accepted = BTreeMap::new();
    accepted.insert((EntityAssetKind::Entity, "a".into(), "a".into()), symbol("a", 3));
    assert!(charge_retained_dependencies(
        &mut EntityDependencyStorageBudget::new(MAX_ENTITY_CATALOG_BYTES),
        &mut accepted
    )
    .is_ok());
    assert_eq!(accepted.values().next().unwrap().charged_dependencies, 3);

    // First source keeps 128 MiB, the second crosses 512 MiB on its dependencies,
    // and a third source must not be visited at all.
    let mut symbols = BTreeMap::new();
    for (name, count) in [("a", 1), ("b", 4), ("c", 4)] {
        symbols.insert(
            (EntityAssetKind::Entity, name.into(), name.into()),
            symbol(name, count),
        );
    }
    let error = charge_retained_dependencies(
        &mut EntityDependencyStorageBudget::new(MAX_ENTITY_CATALOG_BYTES),
        &mut symbols,
    )
    .expect_err("the catalog-sized dependency budget rejects the aggregate");
    assert!(
        format!("{error}").contains("entity dependency storage exceeds catalog budget"),
        "{error}"
    );
    let charged = |name: &str| {
        symbols[&(EntityAssetKind::Entity, name.into(), name.into())].charged_dependencies
    };
    assert_eq!(charged("a"), 1);
    assert!(charged("b") < 4, "failure stops inside the offending source");
    assert_eq!(charged("c"), 0, "no later source is traversed after rejection");
}
