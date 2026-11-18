use super::*;

#[test]
fn legacy_egg_actor_selectors_require_review_when_the_sample_pin_changes() {
    assert_eq!(assets::active_content_registry_protocol(), 2225);
    assert_eq!(
        assets::vanilla_source().commit.as_ref(),
        "4d30ae63750467771b6eccda105f732c6cb77ff7"
    );
}

#[test]
fn pinned_current_villager_eggs_keep_their_actor_sprites() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ROOT") else {
        eprintln!("skipping missing fixture: CINNABAR_VANILLA_ROOT is not set");
        return;
    };
    let root = std::path::PathBuf::from(root);
    for (name, index) in [("villager", 14), ("zombie_villager", 42)] {
        let relative = format!("entity/{name}_v2.entity.json");
        let path = root.join(&relative);
        if !path.exists() {
            eprintln!("skipping missing fixture: {}", path.display());
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let texture_data = Map::from_iter([
            (
                "spawn_egg".into(),
                serde_json::json!({"textures": (0..=index).map(|i| format!("legacy/{i}.png")).collect::<Vec<_>>() }),
            ),
            (
                format!("spawn_egg_{name}"),
                serde_json::json!({"textures":"preview/villager.png"}),
            ),
        ]);
        let legacy_texture = format!("legacy/{index}.png");
        let indices = BTreeMap::from([(legacy_texture.as_str(), 17), ("preview/villager.png", 29)]);
        for (custom, present, expected) in [(true, true, 29), (false, true, 29), (false, false, 29)]
        {
            let mut bytes = bytes.clone();
            if custom {
                bytes.push(b'\n');
            }
            let source = EntityAssetSource {
                path: relative.clone().into(),
                source_bytes: bytes.len() as u32,
                source_sha256: Sha256::digest(&bytes).into(),
            };
            let legacy_path = format!("entity/{name}.entity.json");
            let legacy = serde_json::to_vec(&serde_json::json!({
            "minecraft:client_entity":{"description":{
                "identifier":format!("minecraft:{name}"), "spawn_egg":{"texture":"spawn_egg","texture_index":index}
            }}
        })).unwrap();
            let legacy_source = EntityAssetSource {
                path: legacy_path.clone().into(),
                source_bytes: legacy.len() as u32,
                source_sha256: Sha256::digest(&legacy).into(),
            };
            let mut sources = vec![source];
            let mut payloads =
                BTreeMap::from([(relative.clone().into(), bytes.into_boxed_slice())]);
            if present {
                sources.push(legacy_source);
                payloads.insert(legacy_path.into(), legacy.into_boxed_slice());
            }
            let mut definitions = BTreeMap::new();
            compile(
                &root,
                &payloads,
                &sources,
                &indices,
                &texture_data,
                &mut definitions,
            )
            .unwrap();
            let (_, route) = &definitions[&ItemVisualKey {
                identifier: format!("minecraft:{name}_spawn_egg").into(),
                metadata: 0,
            }];
            assert!(
                matches!(route, ItemVisualDefinitionRoute::Sprite { texture } if texture.source == expected),
                "{name} custom={custom} legacy={present}: {route:?}"
            );
            let (legacy_aux, current_aux) = if name == "villager" {
                (15, 115)
            } else {
                (44, 116)
            };
            let (_, current) = &definitions[&ItemVisualKey {
                identifier: "minecraft:spawn_egg".into(),
                metadata: current_aux,
            }];
            assert_eq!(
                current, route,
                "current generic egg keeps the selected actor"
            );
            if present {
                let (_, legacy) = &definitions[&ItemVisualKey {
                    identifier: "minecraft:spawn_egg".into(),
                    metadata: legacy_aux,
                }];
                assert!(
                    matches!(legacy, ItemVisualDefinitionRoute::Sprite { texture } if texture.source == 29),
                    "legacy generic egg uses the selected current actor: {legacy:?}"
                );
            }
        }
    }
}

#[test]
fn legacy_egg_auxiliaries_select_actors_instead_of_atlas_positions() {
    let actor_specs = [
        ("villager", 15, "legacy_villager", 7),
        ("villager_v2", 115, "current_villager", 8),
        ("zombie_villager", 44, "legacy_zombie", 9),
        ("zombie_villager_v2", 116, "current_zombie", 10),
        ("npc", 51, "npc", 11),
    ];
    let mut payloads = SourcePayloads::new();
    let mut sources = Vec::new();
    let mut texture_data = Map::new();
    let mut indices = BTreeMap::new();
    let paths: Vec<_> = actor_specs
        .iter()
        .map(|(_, _, alias, _)| format!("textures/{alias}.png"))
        .collect();
    let mut definitions = Definitions::new();
    for ((name, aux, alias, source), path) in actor_specs.iter().zip(&paths) {
        let relative = format!("entity/{name}.entity.json");
        let bytes = serde_json::to_vec(&serde_json::json!({
            "minecraft:client_entity":{"description":{
                "identifier":format!("minecraft:{name}"),
                "spawn_egg":{"texture":alias}
            }}
        }))
        .unwrap();
        sources.push(EntityAssetSource {
            path: relative.clone().into(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(&bytes).into(),
        });
        payloads.insert(relative.into(), bytes.into_boxed_slice());
        texture_data.insert((*alias).into(), serde_json::json!({"textures":path}));
        indices.insert(path.as_str(), *source);
        // An atlas array position is a different namespace from an ActorType auxiliary.
        definitions.insert(
            ItemVisualKey {
                identifier: "minecraft:spawn_egg".into(),
                metadata: *aux,
            },
            (
                0,
                ItemVisualDefinitionRoute::Sprite {
                    texture: ItemTextureReference {
                        source: 99,
                        variant: *aux,
                    },
                },
            ),
        );
    }
    compile(
        Path::new("."),
        &payloads,
        &sources,
        &indices,
        &texture_data,
        &mut definitions,
    )
    .unwrap();
    for (_, aux, _, source) in actor_specs {
        let source = match aux {
            15 => 8,
            44 => 10,
            _ => source,
        };
        let (_, route) = &definitions[&ItemVisualKey {
            identifier: "minecraft:spawn_egg".into(),
            metadata: aux,
        }];
        assert!(
            matches!(route,ItemVisualDefinitionRoute::Sprite {texture} if texture.source==source && texture.variant==0),
            "legacy auxiliary {aux} selected an atlas position: {route:?}"
        );
    }
    // An absent replacement keeps the original actor's authored icon.
    let legacy_sources: Vec<_> = sources
        .into_iter()
        .filter(|source| !source.path.ends_with("_v2.entity.json"))
        .collect();
    let mut fallback = Definitions::new();
    compile(
        Path::new("."),
        &payloads,
        &legacy_sources,
        &indices,
        &texture_data,
        &mut fallback,
    )
    .unwrap();
    for (auxiliary, expected) in [(15, 7), (44, 9)] {
        let (_, route) = &fallback[&ItemVisualKey {
            identifier: "minecraft:spawn_egg".into(),
            metadata: auxiliary,
        }];
        assert!(
            matches!(route, ItemVisualDefinitionRoute::Sprite { texture }
            if texture.source == expected)
        );
    }
    assert_eq!(
        definitions[&ItemVisualKey {
            identifier: "minecraft:npc_spawn_egg".into(),
            metadata: 0,
        }],
        definitions[&ItemVisualKey {
            identifier: "minecraft:spawn_egg".into(),
            metadata: 51,
        }]
    );
}
