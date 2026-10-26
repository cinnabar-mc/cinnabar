use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

/// Directional face shading or uniform texture brightness for a carried cube.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CubeLighting {
    #[default]
    Directional,
    Unshaded,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Classification {
    Unshaded,
    Incomplete,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    identifier: Box<str>,
    lighting: Classification,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    schema: u32,
    entries: Vec<Entry>,
}

/// Resolves verified unshaded artwork; incomplete emissive faces keep the directional fallback.
pub fn cube_lighting(identifier: &str) -> CubeLighting {
    static TABLE: OnceLock<BTreeMap<Box<str>, CubeLighting>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let source = source();
        source
            .entries
            .into_iter()
            .map(|entry| {
                let lighting = match entry.lighting {
                    Classification::Unshaded => CubeLighting::Unshaded,
                    Classification::Incomplete => CubeLighting::Directional,
                };
                (entry.identifier, lighting)
            })
            .collect()
    });
    table.get(identifier).copied().unwrap_or_default()
}

/// Validates the canonical classification order before constructing runtime lookups.
fn source() -> Source {
    let source: Source =
        serde_json::from_str(include_str!("../../data/gui-cube-lighting-v2193.json"))
            .expect("valid carried cube lighting source");
    assert_eq!(source.schema, 1);
    assert!(
        source
            .entries
            .windows(2)
            .all(|pair| { pair[0].identifier < pair[1].identifier })
    );
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jack_o_lantern_inventory_faces_keep_full_texture_brightness() {
        assert_eq!(
            cube_lighting("minecraft:lit_pumpkin"),
            CubeLighting::Unshaded
        );
        assert_eq!(
            cube_lighting("minecraft:carved_pumpkin"),
            CubeLighting::Directional
        );
        assert_eq!(
            cube_lighting("minecraft:pumpkin"),
            CubeLighting::Directional
        );
    }

    #[test]
    fn every_emissive_registry_cube_has_an_explicit_lighting_classification() {
        let registry = crate::pinned_block_registry_bytes();
        let protocol = crate::active_content_registry_protocol();
        let records = crate::read_registry_for_protocol(registry, protocol).unwrap();
        let lights = crate::read_light_registry_for_protocol(
            include_bytes!("../../data/block-light-registry-v2193.bin"),
            registry,
            records.len(),
            protocol,
        )
        .unwrap();
        let expected = records
            .iter()
            .zip(lights.iter())
            .filter(|(record, light)| {
                record.model_family == crate::ModelFamily::Cube && light.emission() > 0
            })
            .map(|(record, _)| record.name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let actual = source()
            .entries
            .into_iter()
            .map(|entry| entry.identifier)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(expected, actual, "classify added or changed emissive cubes");
        assert_eq!(cube_lighting("custom:glowstone"), CubeLighting::Directional);
    }
}
