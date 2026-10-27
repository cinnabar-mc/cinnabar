//! Material instances of a custom block's visual components.

use std::collections::HashMap;

use protocol_ids::BlockRenderMethod;

use super::{CustomMaterialInstance, Nbt};

/// Reads every entry of the `materials` table and applies its `mappings` aliases; `None` without
/// a table. Missing or mistyped fields take the client's network defaults.
pub(super) fn instances(component: Option<&Nbt>) -> Option<Box<[CustomMaterialInstance]>> {
    component
        .and_then(|instances| match instances.field("materials") {
            Some(Nbt::Compound(fields)) => Some(fields),
            _ => None,
        })
        .map(|fields| {
            let mut instances: Vec<_> = fields
                .iter()
                .map(|(name, material)| CustomMaterialInstance {
                    name: name.as_str().into(),
                    texture: material
                        .field("texture")
                        .and_then(Nbt::as_str)
                        .unwrap_or_default()
                        .into(),
                    render_method: BlockRenderMethod::from_network(
                        material.field("render_method").and_then(Nbt::as_str),
                    ),
                    tint_method: material
                        .field("tint_method")
                        .and_then(Nbt::as_str)
                        .map(Into::into),
                    ambient_occlusion: match material.field("ambient_occlusion") {
                        Some(Nbt::Float(value)) => *value as f32,
                        _ => 0.0,
                    },
                    packed_bools: match material.field("packed_bools") {
                        Some(Nbt::Byte(value)) => {
                            *value as u8 & CustomMaterialInstance::PACKED_BOOLS_MASK
                        }
                        _ => 0,
                    },
                })
                .collect();
            alias_materials(&mut instances, component);
            instances.into_boxed_slice()
        })
}

/// Copies each `mappings` target instance under its alias name, replacing an instance of that
/// name, as the client builds its material table; an alias to a missing instance is ignored.
/// Mappings apply in order, so a later alias may name an earlier one. Every entry is kept, and
/// a name index keeps the work linear in the server-controlled table sizes.
fn alias_materials(instances: &mut Vec<CustomMaterialInstance>, component: Option<&Nbt>) {
    let Some(Nbt::Compound(mappings)) = component.and_then(|component| component.field("mappings"))
    else {
        return;
    };
    // The first instance of a repeated name is the one a lookup finds.
    let mut index = HashMap::with_capacity(instances.len() + mappings.len());
    for (position, instance) in instances.iter().enumerate() {
        index
            .entry(std::sync::Arc::clone(&instance.name))
            .or_insert(position);
    }
    for (alias, mapping) in mappings {
        let Some(target) = mapping.field("value").and_then(Nbt::as_str) else {
            continue;
        };
        let Some(&source) = index.get(target) else {
            continue;
        };
        let aliased = CustomMaterialInstance {
            name: alias.as_str().into(),
            ..instances[source].clone()
        };
        match index.get(alias.as_str()) {
            Some(&existing) => instances[existing] = aliased,
            None => {
                index.insert(std::sync::Arc::clone(&aliased.name), instances.len());
                instances.push(aliased);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(fields: impl IntoIterator<Item = (String, Nbt)>) -> Nbt {
        Nbt::Compound(fields.into_iter().collect())
    }

    #[test]
    fn large_mapping_tables_keep_every_alias_and_chain_in_order() {
        const COUNT: usize = 20_000;
        let material = |texture: String| compound([("texture".into(), Nbt::String(texture))]);
        let alias = |target: String| compound([("value".into(), Nbt::String(target))]);
        let component = compound([
            (
                "materials".into(),
                compound(
                    (0..COUNT).map(|index| (format!("m{index}"), material(format!("t{index}")))),
                ),
            ),
            (
                "mappings".into(),
                // Each alias names the previous alias, and every other one replaces a material.
                compound((0..COUNT).map(|index| {
                    let name = if index % 2 == 0 {
                        format!("m{index}")
                    } else {
                        format!("a{index}")
                    };
                    let target = if index == 0 {
                        "m1".to_owned()
                    } else if (index - 1) % 2 == 0 {
                        format!("m{}", index - 1)
                    } else {
                        format!("a{}", index - 1)
                    };
                    (name, alias(target))
                })),
            ),
        ]);
        let instances = instances(Some(&component)).unwrap();
        assert_eq!(instances.len(), COUNT + COUNT / 2);
        let texture = |name: &str| {
            instances
                .iter()
                .find(|instance| instance.name.as_ref() == name)
                .map(|instance| instance.texture.as_ref())
        };
        // `m0` takes `m1`'s texture, and every later alias inherits it down the chain.
        assert_eq!(texture("m0"), Some("t1"));
        assert_eq!(texture(&format!("a{}", COUNT - 1)), Some("t1"));
        assert_eq!(texture(&format!("m{}", COUNT - 2)), Some("t1"));
        assert_eq!(texture("m1"), Some("t1"));
    }
}
