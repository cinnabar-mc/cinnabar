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

/// The wearers that draw dedicated baby armor: babies of mobs that publish
/// `variable.use_dedicated_baby_armor`. The pinned zombie-family entities set it; the engine-side
/// rule that turns it on is unverified, so this is the narrowest supported reading.
const BABY_CONDITION: &str = "variable.use_dedicated_baby_armor && query.is_baby";
/// Pack render controller that selects `Geometry.baby` and `Texture.baby` when
/// `variable.use_baby_geo` is set.
const BABY_CONTROLLER: &str = "controller.render.armor.v2";
const BABY_CONTROLLER_SOURCE: &str = "render_controllers/armor.v2.render_controllers.json";
/// Set by a baby variant, which is only selected for a baby wearer.
const BABY_GEOMETRY_FLAG: &str = "variable.use_baby_geo = 1.0;";
const ARMOUR_TEXTURES: &str = "textures/models/armor/";

/// The armour piece a humanoid armour geometry identifier names.
fn armour_piece(geometry: &str) -> Option<&str> {
    ["geometry.player.armor.", "geometry.humanoid.armor."]
        .iter()
        .find_map(|family| geometry.strip_prefix(family))
        .filter(|piece| !piece.is_empty() && !piece.contains('.'))
}

/// The pack texture holding `path` with either raster extension.
fn has_texture(has_source: &impl Fn(&str) -> bool, path: &str) -> bool {
    ["png", "tga"]
        .iter()
        .any(|extension| has_source(&format!("{path}.{extension}")))
}

/// Baby artwork beside an adult armour layer texture such as `textures/models/armor/iron_1`.
/// Chainmail and turtle armor use different material names in the baby artwork.
fn baby_texture(has_source: &impl Fn(&str) -> bool, adult: &str) -> Option<String> {
    let (stem, layer) = adult.strip_prefix(ARMOUR_TEXTURES)?.rsplit_once('_')?;
    if layer.is_empty() || !layer.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let material = match stem {
        "chain" => "chainmail",
        "turtle" => "turtle_scute",
        _ => stem,
    };
    let path = format!("{ARMOUR_TEXTURES}{material}_baby");
    has_texture(has_source, &path).then_some(path)
}

/// Adds a baby branch beside each stock armour definition that lacks one, so a baby wearer of
/// dedicated-armor mobs draws the pack's baby armor geometry and texture instead of the adult
/// pieces. The branch is a separate owner-conditioned definition, so authored definitions and
/// server packs keep precedence. Stock definitions are the plain ones whose armor controller
/// has no baby aliases.
pub(super) fn baby_variants<'a>(
    has_source: impl Fn(&str) -> bool,
    documents: impl Iterator<Item = &'a Value> + Clone,
) -> Result<Vec<(Box<str>, Value)>, AssetError> {
    if !has_source(BABY_CONTROLLER_SOURCE) {
        return Ok(Vec::new());
    }
    let facts =
        assets::facts::ItemFacts::builtin().map_err(|_| invalid("item facts are unavailable"))?;
    let description_of = |document: &'a Value| &document["minecraft:attachable"]["description"];
    // Items already handling babies, or already given a baby branch, keep their definitions.
    let mut handled: BTreeSet<&str> = BTreeSet::new();
    for document in documents.clone() {
        let description = description_of(document);
        let items: Vec<&str> = match description["item"].as_object() {
            Some(items) => items.keys().map(String::as_str).collect(),
            None => description["identifier"].as_str().into_iter().collect(),
        };
        let baby_aware = description["geometry"].get("baby").is_some()
            || description["textures"].get("baby").is_some()
            || description["identifier"]
                .as_str()
                .is_some_and(|identifier| identifier.ends_with(".baby"));
        if baby_aware {
            handled.extend(items);
        }
    }
    let mut missing = Vec::new();
    for template in documents {
        let description = description_of(template);
        let (Some(item), Some(geometry), Some(texture)) = (
            description["identifier"].as_str(),
            description["geometry"]["default"].as_str(),
            description["textures"]["default"].as_str(),
        ) else {
            continue;
        };
        if description.get("item").is_some()
            || handled.contains(item)
            || description["render_controllers"] != serde_json::json!(["controller.render.armor"])
            || !matches!(description["scripts"], Value::Null | Value::Object(_))
            || facts
                .by_identifier(item)
                .is_none_or(|fact| fact.armour_slot.is_none())
        {
            continue;
        }
        let Some(piece) = armour_piece(geometry) else {
            continue;
        };
        if !has_source(&format!("models/entity/baby_armor_{piece}.geo.json")) {
            continue;
        }
        let Some(baby) = baby_texture(&has_source, texture) else {
            continue;
        };
        handled.insert(item);
        let identifier = format!("{item}.baby");
        let mut document = template.clone();
        let description = &mut document["minecraft:attachable"]["description"];
        description["identifier"] = Value::String(identifier.clone());
        description["item"] = serde_json::json!({(item): BABY_CONDITION});
        description["geometry"]["baby"] =
            Value::String(format!("geometry.humanoid.baby.armor.{piece}"));
        description["textures"]["baby"] = Value::String(baby);
        description["render_controllers"] = serde_json::json!([BABY_CONTROLLER]);
        let scripts = &mut description["scripts"]["pre_animation"];
        let mut statements = match scripts.take() {
            Value::Null => Vec::new(),
            Value::Array(statements) => statements,
            statement @ Value::String(_) => vec![statement],
            _ => continue,
        };
        statements.push(Value::String(BABY_GEOMETRY_FLAG.into()));
        *scripts = Value::Array(statements);
        missing.push((
            format!(
                "attachables/_cinnabar/{}.json",
                identifier.replace(':', "/")
            )
            .into(),
            document,
        ));
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

    /// A stock-shaped armour definition: plain controller, adult geometry and layer texture.
    fn stock(item: &str, geometry: &str, texture: &str) -> Value {
        serde_json::json!({"format_version":"1.8.0","minecraft:attachable":{"description":{
            "identifier":item,
            "materials":{"default":"armor","enchanted":"armor_enchanted"},
            "textures":{"default":texture,"enchanted":"textures/misc/enchanted_actor_glint"},
            "geometry":{"default":geometry},
            "scripts":{"parent_setup":"variable.helmet_layer_visible = 0.0;"},
            "render_controllers":["controller.render.armor"]
        }}})
    }

    const BABY_SOURCES: &[&str] = &[
        BABY_CONTROLLER_SOURCE,
        "models/entity/baby_armor_helmet.geo.json",
        "models/entity/baby_armor_leggings.geo.json",
        "textures/models/armor/iron_baby.png",
        "textures/models/armor/turtle_scute_baby.png",
        "textures/models/armor/chainmail_baby.png",
    ];

    #[test]
    fn stock_armour_gains_a_baby_branch_without_replacing_authored_definitions() {
        let helmet = stock(
            "minecraft:iron_helmet",
            "geometry.humanoid.armor.helmet",
            "textures/models/armor/iron_1",
        );
        let leggings = stock(
            "minecraft:iron_leggings",
            "geometry.player.armor.leggings",
            "textures/models/armor/iron_2",
        );
        let turtle = stock(
            "minecraft:turtle_helmet",
            "geometry.humanoid.armor.helmet",
            "textures/models/armor/turtle_1",
        );
        let chainmail = stock(
            "minecraft:chainmail_helmet",
            "geometry.humanoid.armor.helmet",
            "textures/models/armor/chain_1",
        );
        let mut player = helmet.clone();
        let description = &mut player["minecraft:attachable"]["description"];
        description["identifier"] = "minecraft:iron_helmet.player".into();
        description["item"] = serde_json::json!({"minecraft:iron_helmet":"query.owner_identifier == 'minecraft:player'"});
        let documents = vec![helmet, leggings, turtle, chainmail, player];
        let has_source = |path: &str| BABY_SOURCES.contains(&path);
        let generated = baby_variants(has_source, documents.iter()).unwrap();
        // Only the plain definitions branch; the player-conditioned one is left alone.
        assert_eq!(generated.len(), 4);
        for (path, document) in &generated {
            let description = &document["minecraft:attachable"]["description"];
            let identifier = description["identifier"].as_str().unwrap();
            let item = identifier.strip_suffix(".baby").unwrap();
            assert_eq!(
                &**path,
                format!(
                    "attachables/_cinnabar/{}.json",
                    identifier.replace(':', "/")
                )
            );
            assert_eq!(
                description["item"],
                serde_json::json!({(item): BABY_CONDITION})
            );
            assert_eq!(
                description["render_controllers"],
                serde_json::json!([BABY_CONTROLLER])
            );
            let original = documents
                .iter()
                .map(|document| &document["minecraft:attachable"]["description"])
                .find(|original| original["identifier"] == item && original.get("item").is_none())
                .unwrap();
            assert_eq!(
                description["geometry"]["default"],
                original["geometry"]["default"]
            );
            assert_eq!(
                description["textures"]["default"],
                original["textures"]["default"]
            );
            assert_eq!(
                description["textures"]["enchanted"],
                original["textures"]["enchanted"]
            );
            assert_eq!(
                description["scripts"]["parent_setup"],
                original["scripts"]["parent_setup"]
            );
            assert_eq!(
                description["scripts"]["pre_animation"],
                serde_json::json!([BABY_GEOMETRY_FLAG])
            );
            let piece = armour_piece(original["geometry"]["default"].as_str().unwrap()).unwrap();
            assert_eq!(
                description["geometry"]["baby"],
                format!("geometry.humanoid.baby.armor.{piece}")
            );
        }
        let baby_texture_of = |item: &str| {
            let (_, document) = generated
                .iter()
                .find(|(_, document)| {
                    document["minecraft:attachable"]["description"]["identifier"]
                        == format!("{item}.baby")
                })
                .unwrap();
            document["minecraft:attachable"]["description"]["textures"]["baby"].clone()
        };
        assert_eq!(
            baby_texture_of("minecraft:iron_helmet"),
            "textures/models/armor/iron_baby"
        );
        assert_eq!(
            baby_texture_of("minecraft:iron_leggings"),
            "textures/models/armor/iron_baby"
        );
        assert_eq!(
            baby_texture_of("minecraft:turtle_helmet"),
            "textures/models/armor/turtle_scute_baby"
        );
        assert_eq!(
            baby_texture_of("minecraft:chainmail_helmet"),
            "textures/models/armor/chainmail_baby"
        );
        // Their own output, an item that already handles babies, or any missing baby resource
        // leaves nothing to add.
        let mut handled = documents.clone();
        handled.extend(generated.iter().map(|(_, document)| document.clone()));
        assert!(
            baby_variants(has_source, handled.iter())
                .unwrap()
                .is_empty()
        );
        let mut authored_baby = documents.clone();
        authored_baby[0]["minecraft:attachable"]["description"]["textures"]["baby"] =
            "textures/authored_baby".into();
        assert_eq!(
            baby_variants(has_source, authored_baby.iter())
                .unwrap()
                .len(),
            3
        );
        let without = |missing: &'static str| {
            baby_variants(|path| path != missing && has_source(path), documents.iter()).unwrap()
        };
        assert!(without(BABY_CONTROLLER_SOURCE).is_empty());
        assert_eq!(without("models/entity/baby_armor_helmet.geo.json").len(), 1);
        assert_eq!(without("textures/models/armor/iron_baby.png").len(), 2);
        let mut server = documents.clone();
        for document in &mut server {
            document["minecraft:attachable"]["description"]["render_controllers"] =
                serde_json::json!(["controller.render.custom"]);
        }
        assert!(baby_variants(has_source, server.iter()).unwrap().is_empty());
    }
}
