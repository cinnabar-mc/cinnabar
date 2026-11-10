//! Server-advertised mappings from smithing ingredients to trim identifiers.
use std::sync::Arc;

/// An ingredient's item identifier followed by its trim pattern or material identifier.
pub type TrimMapping = (Arc<str>, Arc<str>);

/// A session's TrimData registry; item identifiers are never inferred from suffixes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrimRegistry {
    pub patterns: Arc<[TrimMapping]>,
    pub materials: Arc<[TrimMapping]>,
}

impl TrimRegistry {
    /// Resolves the pattern template and material from the server's current registry.
    pub fn resolve(&self, template: &str, addition: &str) -> Option<crate::ItemTrim> {
        let lookup = |entries: &[TrimMapping], item: &str| {
            let mut matches = entries.iter().filter(|(name, _)| name.as_ref() == item);
            let (_, value) = matches.next()?;
            (matches.all(|(_, next)| next == value) && !value.is_empty()).then(|| Arc::clone(value))
        };
        Some(crate::ItemTrim {
            pattern: lookup(&self.patterns, template)?,
            material: lookup(&self.materials, addition)?,
        })
    }
}

/// Retains the version-matched wire registry without imposing a separate entry limit.
pub(crate) fn normalize(packet: bedrock_protocol_1_26_60::packets::TrimData) -> TrimRegistry {
    TrimRegistry {
        patterns: packet
            .trim_pattern_list
            .into_iter()
            .filter_map(|entry| {
                Some((
                    Arc::from(entry.item_name.to_str().ok()?),
                    Arc::from(entry.pattern_id.to_str().ok()?),
                ))
            })
            .collect(),
        materials: packet
            .trim_material_list
            .into_iter()
            .filter_map(|entry| {
                Some((
                    Arc::from(entry.item_name.to_str().ok()?),
                    Arc::from(entry.material_id.to_str().ok()?),
                ))
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock_protocol_1_26_60::{
        packets::TrimData,
        types::{TrimMaterial, TrimPattern},
    };

    #[test]
    fn trim_data_reaches_inventory_and_resolves_advertised_ingredients() {
        let packet = crate::Packet::new(TrimData {
            trim_pattern_list: vec![TrimPattern {
                item_name: "custom:template".into(),
                pattern_id: "sentry".into(),
            }],
            trim_material_list: vec![TrimMaterial {
                item_name: "custom:material".into(),
                material_id: "gold".into(),
                color: "§6".into(),
            }],
        });
        let Some(crate::WorldEvent::Inventory(crate::InventoryEvent::Trims(registry))) =
            crate::into_world_event(packet, 0).unwrap()
        else {
            panic!("TrimData must be retained");
        };
        assert_eq!(
            registry.resolve("custom:template", "custom:material"),
            Some(crate::ItemTrim {
                pattern: "sentry".into(),
                material: "gold".into()
            })
        );
        assert!(
            registry
                .resolve("custom:template", "minecraft:gold_ingot")
                .is_none()
        );
    }

    #[test]
    fn conflicting_registry_entries_do_not_guess_a_trim() {
        let registry = TrimRegistry {
            patterns: Arc::from([
                ("custom:template".into(), "sentry".into()),
                ("custom:template".into(), "coast".into()),
            ]),
            materials: Arc::from([("custom:material".into(), "gold".into())]),
        };
        assert!(
            registry
                .resolve("custom:template", "custom:material")
                .is_none()
        );
    }
}
