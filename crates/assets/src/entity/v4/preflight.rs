use serde::{Deserialize, Deserializer, de};

use crate::item::{MAX_ITEM_VISUAL_ALIASES, MAX_ITEM_VISUALS};

use super::super::{
    EntityDependencyStorageBudget, MAX_ENTITY_ASSET_SOURCES, MAX_ENTITY_ASSET_SYMBOLS,
    MAX_ENTITY_CATALOG_BYTES, MAX_ENTITY_GEOMETRY_BONES, MAX_ENTITY_GEOMETRY_CUBES,
    MAX_ENTITY_GEOMETRY_TEXTURE_MESHES, MAX_GEOMETRY_LOCATORS,
};
use super::{
    MAX_ENTITY_ANIMATION_ENTRIES, MAX_ENTITY_CONTROLLER_ANIMATIONS, MAX_ENTITY_CONTROLLER_STATES,
    MAX_ENTITY_CONTROLLER_TRANSITIONS, MAX_ENTITY_CONTROLLERS, MAX_ENTITY_RENDER_CANDIDATES,
    MAX_ENTITY_RENDER_LAYERS, MAX_ENTITY_RENDER_SLOTS, MAX_ENTITY_RENDER_VISIBILITY,
    MAX_ENTITY_RIG_BINDINGS, MAX_ENTITY_RIG_CONTROLLERS, MAX_ENTITY_RIG_GEOMETRIES,
    MAX_MOLANG_COLLECTION_ITEMS_TOTAL, MAX_MOLANG_COLLECTIONS, MAX_MOLANG_EXPRESSIONS,
    MAX_MOLANG_OPS,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityCatalogCountProbe {
    interaction_traits: SequenceCount,
    #[serde(rename = "block_visual_count")]
    _block_visual_count: de::IgnoredAny,
    sources: SequenceCount,
    seat_layouts: SequenceCount,
    symbols: SymbolSequenceCount,
    geometries: GeometrySequenceCount,
    animation_clips: SequenceCount,
    animation_channels: SequenceCount,
    animation_keyframes: SequenceCount,
    molang_symbols: SequenceCount,
    molang_expressions: SequenceCount,
    molang_ops: SequenceCount,
    molang_collections: SequenceCount,
    molang_collection_items: SequenceCount,
    controllers: SequenceCount,
    controller_states: SequenceCount,
    controller_animations: SequenceCount,
    controller_transitions: SequenceCount,
    rig_bindings: SequenceCount,
    rig_geometries: SequenceCount,
    rig_animations: SequenceCount,
    rig_controllers: SequenceCount,
    item_use_profiles: SequenceCount,
    item_visuals: SequenceCount,
    item_visual_aliases: SequenceCount,
    render: RenderCountProbe,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderCountProbe {
    layers: SequenceCount,
    slots: SequenceCount,
    candidates: SequenceCount,
    visibility: SequenceCount,
    #[serde(default)]
    material_selections: SequenceCount,
    #[serde(default)]
    geometries: SequenceCount,
    #[serde(default)]
    materials: SequenceCount,
}

#[derive(Default)]
struct SequenceCount(usize);

impl<'de> Deserialize<'de> for SequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = SequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity carrier array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut count = 0usize;
                while sequence.next_element::<de::IgnoredAny>()?.is_some() {
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity carrier array count overflow"))?;
                }
                Ok(SequenceCount(count))
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct SymbolProbe {
    mount_equipment: de::IgnoredAny,
    kind: de::IgnoredAny,
    identifier: de::IgnoredAny,
    source_index: de::IgnoredAny,
    dependencies: DependencySequenceCount,
}

#[derive(Default)]
struct StringByteCount(usize);

impl<'de> Deserialize<'de> for StringByteCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = StringByteCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity dependency identifier")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StringByteCount(value.len()))
            }

            fn visit_borrowed_str<E>(self, value: &'_ str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(value)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StringByteCount(value.len()))
            }
        }

        deserializer.deserialize_str(Visitor)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct DependencyProbe {
    kind: de::IgnoredAny,
    identifier: StringByteCount,
    resolution: de::IgnoredAny,
}

#[derive(Default)]
struct DependencySequenceCount {
    count: usize,
    identifier_bytes: usize,
}

impl<'de> Deserialize<'de> for DependencySequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = DependencySequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity dependency array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut count = 0usize;
                let mut identifier_bytes = 0usize;
                while let Some(dependency) = sequence.next_element::<DependencyProbe>()? {
                    let DependencyProbe {
                        kind: _,
                        identifier,
                        resolution: _,
                    } = dependency;
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity dependency count overflow"))?;
                    identifier_bytes = identifier_bytes
                        .checked_add(identifier.0)
                        .ok_or_else(|| de::Error::custom("entity dependency bytes overflow"))?;
                }
                Ok(DependencySequenceCount {
                    count,
                    identifier_bytes,
                })
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

struct SymbolSequenceCount(usize);

impl<'de> Deserialize<'de> for SymbolSequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::deserialize_with_budget(deserializer, MAX_ENTITY_CATALOG_BYTES)
    }
}

impl SymbolSequenceCount {
    /// Counts all symbols' dependency storage before any typed dependency array is allocated.
    fn deserialize_with_budget<'de, D>(
        deserializer: D,
        limit_bytes: usize,
    ) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor {
            limit_bytes: usize,
        }

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = SymbolSequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity symbol array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut count = 0usize;
                let mut dependency_budget = EntityDependencyStorageBudget::new(self.limit_bytes);
                while let Some(symbol) = sequence.next_element::<SymbolProbe>()? {
                    let SymbolProbe {
                        mount_equipment: _,
                        kind: _,
                        identifier: _,
                        source_index: _,
                        dependencies,
                    } = symbol;
                    if dependency_budget
                        .charge_many(dependencies.count, dependencies.identifier_bytes)
                        .is_err()
                    {
                        return Err(de::Error::custom(
                            "entity dependency storage preflight exceeds catalog budget",
                        ));
                    }
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity symbol count overflow"))?;
                }
                Ok(SymbolSequenceCount(count))
            }
        }

        deserializer.deserialize_seq(Visitor { limit_bytes })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct GeometryProbe {
    identifier: de::IgnoredAny,
    inherits: de::IgnoredAny,
    source_index: de::IgnoredAny,
    texture_width: de::IgnoredAny,
    texture_height: de::IgnoredAny,
    bones: BoneSequenceCount,
    #[serde(default)]
    animation_no_head_bob: Option<de::IgnoredAny>,
    #[serde(default)]
    visible_bounds: Option<de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct BoneProbe {
    name: de::IgnoredAny,
    parent: de::IgnoredAny,
    pivot: de::IgnoredAny,
    rotation: de::IgnoredAny,
    #[serde(default)]
    bind_pose_rotation: Option<de::IgnoredAny>,
    mirror: de::IgnoredAny,
    inflate: de::IgnoredAny,
    never_render: de::IgnoredAny,
    reset: de::IgnoredAny,
    #[serde(default)]
    binding: Option<de::IgnoredAny>,
    #[serde(default)]
    texture_meshes: SequenceCount,
    #[serde(default)]
    poly_mesh: Option<de::IgnoredAny>,
    cubes: SequenceCount,
    #[serde(default)]
    locators: SequenceCount,
}

struct BoneSequenceCount;

impl<'de> Deserialize<'de> for BoneSequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = BoneSequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity geometry bone array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut bones = 0usize;
                let mut cubes = 0usize;
                let mut texture_meshes = 0usize;
                let mut locators = 0usize;
                while let Some(bone) = sequence.next_element::<BoneProbe>()? {
                    let BoneProbe {
                        name: _,
                        parent: _,
                        pivot: _,
                        rotation: _,
                        bind_pose_rotation: _,
                        mirror: _,
                        inflate: _,
                        never_render: _,
                        reset: _,
                        binding: _,
                        texture_meshes: bone_meshes,
                        poly_mesh: _,
                        cubes: bone_cubes,
                        locators: bone_locators,
                    } = bone;
                    bones = bones
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity bone count overflow"))?;
                    cubes = cubes
                        .checked_add(bone_cubes.0)
                        .ok_or_else(|| de::Error::custom("entity cube count overflow"))?;
                    texture_meshes = texture_meshes
                        .checked_add(bone_meshes.0)
                        .ok_or_else(|| de::Error::custom("entity texture mesh count overflow"))?;
                    locators = locators
                        .checked_add(bone_locators.0)
                        .ok_or_else(|| de::Error::custom("entity locator count overflow"))?;
                    if bones > MAX_ENTITY_GEOMETRY_BONES
                        || cubes > MAX_ENTITY_GEOMETRY_CUBES
                        || texture_meshes > MAX_ENTITY_GEOMETRY_TEXTURE_MESHES
                        || locators > MAX_GEOMETRY_LOCATORS
                    {
                        return Err(de::Error::custom(
                            "entity geometry subarray count preflight exceeds bound",
                        ));
                    }
                }
                Ok(BoneSequenceCount)
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

struct GeometrySequenceCount(usize);

impl<'de> Deserialize<'de> for GeometrySequenceCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = GeometrySequenceCount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an entity geometry array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut count = 0usize;
                while let Some(geometry) = sequence.next_element::<GeometryProbe>()? {
                    let GeometryProbe {
                        identifier: _,
                        inherits: _,
                        source_index: _,
                        texture_width: _,
                        texture_height: _,
                        bones: _,
                        animation_no_head_bob: _,
                        visible_bounds: _,
                    } = geometry;
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("entity geometry count overflow"))?;
                }
                Ok(GeometrySequenceCount(count))
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

pub(crate) fn payload_counts(bytes: &[u8]) -> Result<[usize; 7], serde_json::Error> {
    let counts: EntityCatalogCountProbe = serde_json::from_slice(bytes)?;
    let bounded = [
        (counts.interaction_traits.0, MAX_ENTITY_ASSET_SYMBOLS * 2),
        (counts.sources.0, MAX_ENTITY_ASSET_SOURCES),
        (counts.symbols.0, MAX_ENTITY_ASSET_SYMBOLS),
        (counts.seat_layouts.0, MAX_ENTITY_ASSET_SYMBOLS),
        (counts.animation_clips.0, MAX_ENTITY_ANIMATION_ENTRIES),
        (counts.animation_channels.0, MAX_ENTITY_ANIMATION_ENTRIES),
        (counts.animation_keyframes.0, MAX_ENTITY_ANIMATION_ENTRIES),
        (counts.molang_symbols.0, MAX_MOLANG_EXPRESSIONS),
        (counts.molang_expressions.0, MAX_MOLANG_EXPRESSIONS),
        (counts.molang_ops.0, MAX_MOLANG_OPS),
        (counts.molang_collections.0, MAX_MOLANG_COLLECTIONS),
        (
            counts.molang_collection_items.0,
            MAX_MOLANG_COLLECTION_ITEMS_TOTAL,
        ),
        (counts.controllers.0, MAX_ENTITY_CONTROLLERS),
        (counts.controller_states.0, MAX_ENTITY_CONTROLLER_STATES),
        (
            counts.controller_animations.0,
            MAX_ENTITY_CONTROLLER_ANIMATIONS,
        ),
        (
            counts.controller_transitions.0,
            MAX_ENTITY_CONTROLLER_TRANSITIONS,
        ),
        (counts.rig_bindings.0, MAX_ENTITY_RIG_BINDINGS),
        (counts.rig_geometries.0, MAX_ENTITY_RIG_GEOMETRIES),
        (counts.rig_animations.0, MAX_ENTITY_ANIMATION_ENTRIES),
        (counts.rig_controllers.0, MAX_ENTITY_RIG_CONTROLLERS),
        (counts.render.layers.0, MAX_ENTITY_RENDER_LAYERS),
        (counts.render.slots.0, MAX_ENTITY_RENDER_SLOTS),
        (counts.render.candidates.0, MAX_ENTITY_RENDER_CANDIDATES),
        (
            counts.render.material_selections.0,
            MAX_ENTITY_RENDER_CANDIDATES,
        ),
        (counts.render.visibility.0, MAX_ENTITY_RENDER_VISIBILITY),
        (counts.render.geometries.0, MAX_ENTITY_RENDER_CANDIDATES),
        (
            counts.render.materials.0,
            super::material::MAX_ENTITY_MATERIAL_DEFINITIONS,
        ),
        (counts.item_use_profiles.0, MAX_ITEM_VISUALS * 2),
        (counts.item_visuals.0, MAX_ITEM_VISUALS),
        (counts.item_visual_aliases.0, MAX_ITEM_VISUAL_ALIASES),
    ];
    if bounded.iter().any(|(count, maximum)| count > maximum) {
        return Err(<serde_json::Error as de::Error>::custom(
            "entity carrier count preflight exceeds a retained-section bound",
        ));
    }
    Ok([
        counts.sources.0,
        counts.symbols.0,
        counts.geometries.0,
        counts.animation_clips.0,
        counts.controllers.0,
        counts.rig_bindings.0,
        counts.item_visuals.0,
    ])
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn dependency_preflight_budget_is_shared_across_symbols() {
        let dependency = serde_json::json!({"kind":null,"identifier":"x","resolution":null});
        let symbol = serde_json::json!({
            "mount_equipment":null,"kind":null,"identifier":null,"source_index":null,
            "dependencies":[dependency]
        });
        let bytes = serde_json::to_vec(&vec![symbol; 3]).unwrap();
        let entry_bytes = std::mem::size_of::<crate::EntityDependency>() + 1;
        let mut decoder = serde_json::Deserializer::from_slice(&bytes);
        let accepted = SymbolSequenceCount::deserialize_with_budget(&mut decoder, entry_bytes * 3)
            .expect("the exact shared storage budget is admissible");
        assert_eq!(accepted.0, 3);
        let mut decoder = serde_json::Deserializer::from_slice(&bytes);
        let error = SymbolSequenceCount::deserialize_with_budget(&mut decoder, entry_bytes * 2)
            .err()
            .expect("separate symbols must charge the same budget");
        assert!(error.to_string().contains("dependency storage preflight"));
    }

    #[test]
    fn geometry_probe_tracks_all_serialized_geometry_fields_and_remains_strict() {
        use crate::{EntityGeometry, EntityGeometryBounds, EntityGeometryScalar};

        // An exhaustive literal makes a new geometry field require a schema-test decision.
        let geometry = EntityGeometry {
            identifier: "geometry.schema_fixture".into(),
            inherits: None,
            source_index: 0,
            texture_width: 16,
            texture_height: 16,
            bones: Box::new([]),
            animation_no_head_bob: true,
            visible_bounds: Some(EntityGeometryBounds {
                center: [EntityGeometryScalar::ZERO; 3],
                half_extents: [EntityGeometryScalar::new(1.0).unwrap(); 3],
            }),
        };
        let mut value = serde_json::to_value(&geometry).unwrap();
        assert!(serde_json::from_value::<GeometryProbe>(value.clone()).is_ok());
        value
            .as_object_mut()
            .unwrap()
            .remove("animation_no_head_bob");
        value.as_object_mut().unwrap().remove("visible_bounds");
        assert!(serde_json::from_value::<GeometryProbe>(value.clone()).is_ok());
        value["unknown_geometry_field"] = true.into();
        assert!(serde_json::from_value::<GeometryProbe>(value).is_err());
    }
    #[test]
    fn locator_arrays_are_bounded_across_bones_before_payload_deserialization() {
        let mut bone = serde_json::json!({
            "name":"root", "parent":null, "pivot":null, "rotation":null,
            "mirror":null, "inflate":null, "never_render":null, "reset":null,
            "cubes":[], "locators":vec![serde_json::Value::Null; MAX_GEOMETRY_LOCATORS]
        });
        assert!(
            serde_json::from_value::<BoneSequenceCount>(serde_json::json!([bone.clone()])).is_ok()
        );
        assert!(
            serde_json::from_value::<BoneSequenceCount>(serde_json::json!([
                bone.clone(),
                bone.clone()
            ]))
            .is_err()
        );
        bone["locators"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::Null);
        assert!(serde_json::from_value::<BoneSequenceCount>(serde_json::json!([bone])).is_err());
    }

    #[test]
    fn review_render_arrays_are_bounded_before_payload_deserialization() {
        let mut root = serde_json::Map::new();
        root.insert("block_visual_count".into(), serde_json::json!(0));
        for field in [
            "interaction_traits",
            "seat_layouts",
            "item_use_profiles",
            "sources",
            "symbols",
            "geometries",
            "animation_clips",
            "animation_channels",
            "animation_keyframes",
            "molang_symbols",
            "molang_expressions",
            "molang_ops",
            "molang_collections",
            "molang_collection_items",
            "controllers",
            "controller_states",
            "controller_animations",
            "controller_transitions",
            "rig_bindings",
            "rig_geometries",
            "rig_animations",
            "rig_controllers",
            "item_visuals",
            "item_visual_aliases",
        ] {
            root.insert(field.into(), serde_json::json!([]));
        }
        root.insert("render".into(), serde_json::json!({"layers":vec![serde_json::Value::Null; super::super::MAX_ENTITY_RENDER_LAYERS+1], "slots":[],"candidates":[],"visibility":[]}));
        assert!(payload_counts(&serde_json::to_vec(&root).unwrap()).is_err());
    }
}
