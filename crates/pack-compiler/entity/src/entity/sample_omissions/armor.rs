//! Completes copper armour from the pack's standard armour definitions and artwork.

use super::*;
use assets::{facts::ItemTier, item::armour::ItemArmourSlot};

/// Reuses a standard definition for each missing copper piece, preserving authored overrides.
pub(super) fn missing_bindings<'a>(
    has_source: impl Fn(&str) -> bool,
    authored: impl Iterator<Item = &'a Value> + Clone,
) -> Result<Vec<(Box<str>, Value)>, AssetError> {
    let facts =
        assets::facts::ItemFacts::builtin().map_err(|_| invalid("item facts are unavailable"))?;
    let claimed: BTreeSet<&str> = authored
        .clone()
        .flat_map(|document| {
            let description = &document["minecraft:attachable"]["description"];
            match description["item"].as_object() {
                Some(items) => items.keys().map(String::as_str).collect::<Vec<_>>(),
                None => description["identifier"].as_str().into_iter().collect(),
            }
        })
        .collect();
    let mut missing = Vec::new();
    for record in &facts.payload().records {
        if record.fact.tier != Some(ItemTier::Copper)
            || claimed.contains(record.identifier.as_ref())
        {
            continue;
        }
        let Some(slot) = record.fact.armour_slot else {
            continue;
        };
        let (piece, layer) = match slot {
            ItemArmourSlot::Head => ("helmet", 1),
            ItemArmourSlot::Chest => ("chestplate", 1),
            ItemArmourSlot::Legs => ("leggings", 2),
            ItemArmourSlot::Feet => ("boots", 1),
        };
        let texture = format!("textures/models/armor/copper_{layer}");
        if !["png", "tga"]
            .iter()
            .any(|extension| has_source(&format!("{texture}.{extension}")))
        {
            continue;
        }
        let Some(template) = authored.clone().find(|document| {
            let description = &document["minecraft:attachable"]["description"];
            description.get("item").is_none()
                && description["materials"]["default"] == "armor"
                && description["textures"]["default"].is_string()
                && ["geometry.player.armor", "geometry.humanoid.armor"]
                    .iter()
                    .any(|family| description["geometry"]["default"] == format!("{family}.{piece}"))
        }) else {
            continue;
        };
        let Some(source_item) =
            template["minecraft:attachable"]["description"]["identifier"].as_str()
        else {
            continue;
        };
        let owners = authored.clone().filter(|document| {
            let description = &document["minecraft:attachable"]["description"];
            description["textures"]["default"].is_string()
                && description["item"]
                    .as_object()
                    .is_some_and(|items| items.len() == 1 && items.contains_key(source_item))
        });
        for (index, template) in std::iter::once(template).chain(owners).enumerate() {
            let identifier = if index == 0 {
                record.identifier.to_string()
            } else {
                format!("{}.owner_{index}", record.identifier)
            };
            let mut document = template.clone();
            let description = &mut document["minecraft:attachable"]["description"];
            description["identifier"] = Value::String(identifier.clone());
            description["textures"]["default"] = Value::String(texture.clone());
            if let Some(items) = description.get_mut("item").and_then(Value::as_object_mut) {
                let condition = items
                    .remove(source_item)
                    .expect("the template owns this item");
                items.insert(record.identifier.to_string(), condition);
            }
            missing.push((
                format!(
                    "attachables/_cinnabar/{}.json",
                    identifier.replace(':', "/")
                )
                .into(),
                document,
            ));
        }
    }
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_copper_armour_reuses_each_present_standard_definition() {
        let facts = assets::facts::ItemFacts::builtin().unwrap();
        let pieces: Vec<_> = facts
            .payload()
            .records
            .iter()
            .filter(|record| {
                record.fact.tier == Some(ItemTier::Copper) && record.fact.armour_slot.is_some()
            })
            .collect();
        assert!(!pieces.is_empty());
        let templates: Vec<_> = pieces
            .iter()
            .map(|record| {
                let piece = record.identifier.strip_prefix("minecraft:copper_").unwrap();
                serde_json::json!({"format_version":"1.8.0","minecraft:attachable":{"description":{
                    "identifier":format!("fixture:{piece}"),
                    "materials":{"default":"armor","enchanted":"armor_enchanted"},
                    "geometry":{"default":format!("geometry.player.armor.{piece}")},
                    "textures":{"default":"textures/fixture","enchanted":"textures/fixture_glint"},
                    "scripts":{"parent_setup":"variable.fixture = 1;"},
                    "render_controllers":["controller.render.fixture"]
                }}})
            })
            .collect();
        let generated = missing_bindings(|_| true, templates.iter()).unwrap();
        assert_eq!(generated.len(), pieces.len());
        for (_, document) in &generated {
            let description = &document["minecraft:attachable"]["description"];
            let item = description["identifier"].as_str().unwrap();
            let slot = facts.by_identifier(item).unwrap().armour_slot.unwrap();
            let expected = if slot == ItemArmourSlot::Legs { 2 } else { 1 };
            assert_eq!(
                description["textures"]["default"],
                format!("textures/models/armor/copper_{expected}")
            );
            assert_eq!(
                description["textures"]["enchanted"],
                "textures/fixture_glint"
            );
            assert_eq!(
                description["scripts"]["parent_setup"],
                "variable.fixture = 1;"
            );
        }
        assert!(
            missing_bindings(|_| false, templates.iter())
                .unwrap()
                .is_empty()
        );
        assert!(missing_bindings(|_| true, [].iter()).unwrap().is_empty());
        let source_item = templates[0]["minecraft:attachable"]["description"]["identifier"]
            .as_str()
            .unwrap();
        let condition = "query.owner_identifier == 'minecraft:player'";
        let mut owner = templates[0].clone();
        let description = &mut owner["minecraft:attachable"]["description"];
        description["identifier"] = Value::String(format!("{source_item}.player"));
        description["item"] = serde_json::json!({(source_item):condition});
        description["scripts"]["animate"] = serde_json::json!(["offset"]);
        description["animations"] = serde_json::json!({"offset":"animation.fixture.offset"});
        let mut variants = templates.clone();
        variants.push(owner);
        let generated_variants = missing_bindings(|_| true, variants.iter()).unwrap();
        assert_eq!(generated_variants.len(), pieces.len() + 1);
        let owner = generated_variants
            .iter()
            .map(|(_, document)| &document["minecraft:attachable"]["description"])
            .find(|description| description.get("item").is_some())
            .unwrap();
        assert_eq!(
            owner["item"],
            serde_json::json!({(pieces[0].identifier.as_ref()):condition})
        );
        assert_eq!(owner["scripts"]["animate"], serde_json::json!(["offset"]));
        assert_eq!(owner["animations"]["offset"], "animation.fixture.offset");
        let mut malformed = templates.clone();
        for document in &mut malformed {
            document["minecraft:attachable"]["description"]["textures"] =
                Value::String("invalid".into());
        }
        assert!(
            missing_bindings(|_| true, malformed.iter())
                .unwrap()
                .is_empty()
        );
        let mut conditional = templates.clone();
        conditional.push(serde_json::json!({"minecraft:attachable":{"description":{
            "identifier":"fixture:disabled","item":{(pieces[0].identifier.as_ref()):"0"}
        }}}));
        let remaining = missing_bindings(|_| true, conditional.iter()).unwrap();
        assert_eq!(remaining.len(), pieces.len() - 1);
        assert!(remaining.iter().all(|(_, document)| {
            document["minecraft:attachable"]["description"]["identifier"]
                != pieces[0].identifier.as_ref()
        }));
        let mut authored = templates;
        authored.extend(generated.into_iter().map(|(_, document)| document));
        assert!(
            missing_bindings(|_| true, authored.iter())
                .unwrap()
                .is_empty()
        );
    }
}
