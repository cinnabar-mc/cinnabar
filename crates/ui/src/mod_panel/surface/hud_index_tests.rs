//! Control patches admitted through a [`HudIndex`] match whole-document validation exactly.

use super::*;
use crate::mod_panel::surface::{SurfacePatch, backdrop, definitions, hud_control};
use serde_json::json;

/// SplitMix64, so a failing sequence replays from its seed alone.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value below `bound`.
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    /// True `percent` times in a hundred.
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// Cards in the representative document.
const CARDS: usize = 6;

/// A status card: a rounded background and one bound value row, as a first-party HUD module
/// draws one.
fn card(card: usize, alpha: f64) -> Value {
    let value = format!("#v{card}");
    json!({"type":"panel","size":[60,12],"alpha":alpha,"controls":[
        {"background@hud.bg":{}},
        {"value@hud.ls":{"text":value,"bindings":[{"binding_name":value}]}}]})
}

/// The root's instance of card `card`.
fn instance(card: usize, x: usize, y: usize) -> Value {
    json!({format!("g{card}@hud.c{card}"):{"anchor_from":"top_left","anchor_to":"top_left",
        "offset":[x,y]}})
}

/// A representative HUD document: shared label and background templates, cards instancing
/// them, a parameterized template, an unread definition and inline root children.
fn representative() -> Surface {
    let mut document = json!({"namespace":"hud",
        "lb":{"type":"label","color":[1,1,1],"font_scale_factor":0.8},
        "ls@hud.lb":{"size":["default","default"]},
        "bg":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":["100%","100%"],
            "radius":3,"color":[0,0,0,0.4]},
        "tpl":{"type":"panel","size":[40,10],"$caption|default":"x",
            "controls":[{"t@hud.ls":{"text":"$caption"}}]},
        "spare":{"type":"panel","size":[4,4]}});
    let mut children: Vec<Value> = (0..CARDS).map(|i| instance(i, 8, 8 + 14 * i)).collect();
    children.push(json!({"w":{"type":"label","text":"label","offset":[100,100]}}));
    children.push(json!({"t0@hud.tpl":{"$caption":"hi","offset":[100,120]}}));
    document["root"] = json!({"type":"panel","size":["100%","100%"],"controls":children});
    for i in 0..CARDS {
        document[format!("c{i}")] = card(i, 1.0);
    }
    Surface {
        screen: "hud.root".into(),
        document: document.to_string(),
        bindings: (0..CARDS)
            .map(|i| {
                (
                    format!("#v{i}"),
                    crate::mod_panel::SurfaceValue::Text(i.to_string()),
                )
            })
            .collect(),
        transitions: Default::default(),
        collections: Default::default(),
    }
}

/// A patch the generator made, and whether an indexed document should admit it without
/// whole-document validation when that validation accepts it.
struct Generated {
    patch: Value,
    incremental: bool,
}

/// One random control patch to `document`, bound to `bound`: representative edits and the
/// invalid ones each rule exists for, sometimes with binding changes and removals.
fn generate(rng: &mut Rng, document: &Value, bound: &[String], serial: usize) -> Generated {
    let object = document.as_object().expect("document object");
    let locals: Vec<(&String, String)> = object
        .keys()
        .filter(|key| *key != "namespace")
        .map(|key| (key, key.split('@').next().unwrap().to_owned()))
        .collect();
    let key_of = |local: &str| {
        locals
            .iter()
            .find(|(_, name)| name == local)
            .map(|(key, _)| (*key).clone())
    };
    let root = document["root"].clone();
    let entries = root["controls"].as_array().cloned().unwrap_or_default();
    let with_entries = |entries: Vec<Value>| {
        let mut root = root.clone();
        root["controls"] = Value::Array(entries);
        root
    };
    let card_index = rng.below(CARDS);
    let card_name = format!("c{card_index}");
    let other = format!("c{}", rng.below(CARDS));
    let mut controls = Map::new();
    let mut incremental = true;
    match rng.below(27) {
        // A card's own properties change, as a module setting or a scale slider changes them.
        0..=3 => {
            let mut body = card(card_index, rng.below(100) as f64 / 100.);
            if rng.chance(30) {
                body["size"] = json!([20 + rng.below(80), 8 + rng.below(20)]);
            }
            if rng.chance(20) {
                body["controls"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"extra@hud.ls":{"text":"e","offset":[0, rng.below(9)]}}));
            }
            controls.insert(key_of(&card_name).unwrap_or(card_name), body);
        }
        // A drag moves one root child.
        4..=7 if !entries.is_empty() => {
            let mut entries = entries;
            let position = rng.below(entries.len());
            if let Some(body) = entries[position]
                .as_object_mut()
                .and_then(|named| named.values_mut().next())
                && body.is_object()
            {
                body["offset"] = json!([rng.below(300), rng.below(200)]);
            }
            controls.insert("root".into(), with_entries(entries));
        }
        // Root children come, go and reorder.
        8 => {
            let mut entries = entries;
            let position = rng.below(entries.len() + 1);
            entries.insert(
                position,
                instance(card_index, rng.below(300), rng.below(200)),
            );
            controls.insert("root".into(), with_entries(entries));
        }
        9 if !entries.is_empty() => {
            let mut entries = entries;
            entries.remove(rng.below(entries.len()));
            controls.insert("root".into(), with_entries(entries));
        }
        10 if entries.len() > 1 => {
            let mut entries = entries;
            let (a, b) = (rng.below(entries.len()), rng.below(entries.len()));
            entries.swap(a, b);
            controls.insert("root".into(), with_entries(entries));
        }
        // A new definition, read by a new root child or by nothing.
        11 => {
            let name = format!("extra{serial}");
            controls.insert(
                name.clone(),
                json!({"type":"panel","size":[10,10],"controls":[{"l@hud.ls":{"text":"n"}}]}),
            );
            if rng.chance(60) {
                let mut entries = entries;
                entries.push(json!({format!("e{serial}@hud.{name}"):{}}));
                controls.insert("root".into(), with_entries(entries));
            }
        }
        // Removing a definition, often one something still reads.
        12 => {
            let (key, _) = &locals[rng.below(locals.len())];
            controls.insert((*key).clone(), Value::Null);
        }
        // A definition changes its base, possibly into a cycle.
        13 => {
            let base = ["bg", "lb", "spare", other.as_str()][rng.below(4)];
            controls.insert(format!("{card_name}@hud.{base}"), card(card_index, 0.5));
            if let Some(key) = key_of(&card_name).filter(|key| key.contains('@')) {
                controls.remove(&key);
            }
        }
        // A card instances another card, which cycles when that card instances it back.
        14 => {
            let mut body = card(card_index, 1.);
            body["controls"]
                .as_array_mut()
                .unwrap()
                .push(json!({format!("inner@hud.{other}"):{}}));
            controls.insert(key_of(&card_name).unwrap_or(card_name), body);
        }
        // Rejected renderers, input and placeholders.
        15 => {
            let child = match rng.below(5) {
                0 => json!({"type":"custom","renderer":"unknown"}),
                1 => json!({"type":"custom","renderer":hud_control::RENDERER,"control":"c0"}),
                2 => json!({"type":"custom","renderer":backdrop::GLASS_RENDERER}),
                3 => json!({"type":"button","button_mappings":[{"from_button_id":
                    "button.menu_select","mapping_type":"pressed","to_button_id":"mod.control:0"}]}),
                _ => json!({"type":"panel","cinnabar_collection":"missing"}),
            };
            let mut body = card(card_index, 1.);
            body["controls"]
                .as_array_mut()
                .unwrap()
                .push(json!({"bad":child}));
            controls.insert(key_of(&card_name).unwrap_or(card_name), body);
        }
        // Replicated parameter bytes approach or pass the expanded property bound.
        16 => {
            let dollars = 100 + rng.below(700);
            controls.insert(
                "big".into(),
                json!({"type":"label","$p|default":"y".repeat(1000),
                    "text":format!("z{}", "$p".repeat(dollars))}),
            );
            let mut entries = entries;
            for copy in 0..1 + rng.below(4) {
                entries.push(json!({format!("big{serial}_{copy}@hud.big"):{}}));
            }
            controls.insert("root".into(), with_entries(entries));
        }
        // Wide definitions approach the authored node budget; read ones the resolved budget.
        17 if rng.chance(25) => {
            let name = format!("wide{serial}");
            let leaves = 600 + rng.below(700);
            let children: Vec<Value> = (0..leaves)
                .map(|leaf| json!({format!("n{leaf}"):{"type":"panel"}}))
                .collect();
            controls.insert(name.clone(), json!({"type":"panel","controls":children}));
            if rng.chance(50) {
                let mut entries = entries;
                for copy in 0..1 + rng.below(4) {
                    entries.push(json!({format!("w{serial}_{copy}@hud.{name}"):{}}));
                }
                controls.insert("root".into(), with_entries(entries));
            }
            incremental = false;
        }
        // Nesting near the depth limit, alone and once instanced below the root.
        18 => {
            let mut body = json!({"type":"panel"});
            for _ in 0..25 + rng.below(12) {
                body = json!({"type":"panel","controls":[{"n":body}]});
            }
            controls.insert("deep".into(), body);
            if rng.chance(60) {
                let mut entries = entries;
                entries.push(json!({format!("d{serial}@hud.deep"):{}}));
                controls.insert("root".into(), with_entries(entries));
            }
        }
        // A string with `@` could name an animation after substitution: never indexed.
        19 => {
            controls.insert(
                "fade".into(),
                json!({"anim_type":"alpha","from":"$evil","to":1,"duration":1}),
            );
            let mut body = card(card_index, 1.);
            body["$a"] = json!("@hud.fade");
            body["alpha"] = json!("$a");
            controls.insert(key_of(&card_name).unwrap_or(card_name), body);
            incremental = false;
        }
        // The root's own properties change, sometimes into a root that cannot draw.
        20 => {
            let mut root = root.clone();
            match rng.below(5) {
                0 => root["size"] = json!([100 + rng.below(100), "100%"]),
                // The same root body inheriting a base that hides it or gives it input.
                1 if key_of("root").as_deref() == Some("root") => {
                    let base = if rng.chance(50) {
                        json!({"ignored":true})
                    } else {
                        json!({"button_mappings":[{"from_button_id":"button.menu_select",
                            "mapping_type":"pressed","to_button_id":"mod.control:0"}]})
                    };
                    controls.insert("rootbase".into(), base);
                    controls.insert("root@hud.rootbase".into(), root);
                    return Generated {
                        patch: json!({"controls":controls}),
                        incremental: false,
                    };
                }
                2 => root["ignored"] = json!(true),
                3 => {
                    root["type"] = json!("custom");
                    root["renderer"] = json!("unknown");
                }
                _ => {
                    root["$caption"] = json!("from the root");
                    root["controls"] = Value::Array(entries);
                }
            }
            controls.insert("root".into(), root);
            incremental = false;
        }
        // Many instances of a large template approach or pass the resolved node budget.
        25 if rng.chance(30) => {
            let leaves: Vec<Value> = (0..299)
                .map(|leaf| json!({format!("n{leaf}"):{"type":"panel"}}))
                .collect();
            controls.insert("block".into(), json!({"type":"panel","controls":leaves}));
            let mut entries = entries;
            for copy in 0..10 + rng.below(7) {
                entries.push(json!({format!("k{serial}_{copy}@hud.block"):{}}));
            }
            controls.insert("root".into(), with_entries(entries));
            incremental = false;
        }
        // Malformed patches.
        21 => {
            let (key, value) = match rng.below(5) {
                0 => ("namespace", json!({})),
                1 => ("missing", Value::Null),
                2 => ("c0", json!([])),
                3 => ("x@foreign.y", json!({})),
                _ => ("a.b", json!({})),
            };
            controls.insert(key.into(), value);
        }
        // `ignored` children resolve to nothing.
        22 if !entries.is_empty() => {
            let mut entries = entries;
            let position = rng.below(entries.len());
            if let Some(body) = entries[position]
                .as_object_mut()
                .and_then(|named| named.values_mut().next())
                && body.is_object()
            {
                body["ignored"] = json!(rng.chance(50));
            }
            controls.insert("root".into(), with_entries(entries));
        }
        // Template parameters, and a property naming no declared parameter.
        23 => {
            let mut entries = entries;
            let caption = if rng.chance(20) {
                "$missing"
            } else {
                "caption"
            };
            entries.push(json!({format!("t{serial}@hud.tpl"):{"$caption":caption}}));
            let mut tpl = json!({"type":"panel","size":[40,10],"$caption|default":"x",
                "controls":[{"t@hud.ls":{"text":"$caption"}}]});
            if rng.chance(20) {
                tpl["controls"][0]["t@hud.ls"]["text"] = json!("$undeclared");
            }
            controls.insert("tpl".into(), tpl);
            controls.insert("root".into(), with_entries(entries));
        }
        // Several definitions at once.
        24 => {
            for card in [card_index, rng.below(CARDS)] {
                let name = format!("c{card}");
                controls.insert(
                    key_of(&name).unwrap_or(name),
                    self::card(card, rng.below(10) as f64 / 10.),
                );
            }
        }
        _ => {
            controls.insert(
                key_of(&card_name).unwrap_or(card_name),
                card(card_index, 0.25),
            );
        }
    }
    let mut patch = json!({"controls":controls});
    if rng.chance(15) {
        let name = if rng.chance(20) {
            "bad name".to_owned()
        } else {
            format!("#v{}", rng.below(CARDS))
        };
        patch["binding_changes"] = json!({name: rng.below(9).to_string()});
    }
    // Bound names go, sometimes twice, unbound or also set by the same patch.
    if rng.chance(20) {
        let removed: Vec<String> = (0..1 + rng.below(2))
            .map(|_| match rng.below(5) {
                0 => "#unbound".to_owned(),
                _ if bound.is_empty() => format!("#v{}", rng.below(CARDS)),
                _ => bound[rng.below(bound.len())].clone(),
            })
            .collect();
        patch["remove_bindings"] = json!(removed);
    }
    // Binding-only patches keep the index for the patches after them.
    if rng.chance(5) {
        patch.as_object_mut().unwrap().remove("controls");
    }
    Generated { patch, incremental }
}

/// Asserts that an index chained through patches holds what one built afresh for the same
/// surface holds.
fn assert_consistent(chained: &HudIndex, surface: &Surface) {
    let fresh = HudIndex::new(surface).expect("an admitted surface validates");
    let (Some(chained), Some(fresh)) = (chained.0.as_deref(), fresh.0.as_deref()) else {
        assert_eq!(chained.0.is_some(), fresh.0.is_some(), "index presence");
        return;
    };
    assert_eq!(chained.document, fresh.document);
    assert_eq!(
        chained.resolved_nodes, fresh.resolved_nodes,
        "resolved nodes"
    );
    assert_eq!(
        chained.authored_nodes, fresh.authored_nodes,
        "authored nodes"
    );
    assert_eq!(chained.parameter_bytes, fresh.parameter_bytes);
    assert_eq!(chained.readers, fresh.readers);
    for (name, bytes) in &fresh.expanded {
        assert_eq!(
            chained.expanded.get(name),
            Some(bytes),
            "expanded bytes of {name}"
        );
    }
    let texts = |index: &Index| -> Vec<String> {
        index.parts.iter().map(|p| p.entry.text.clone()).collect()
    };
    assert_eq!(texts(chained), texts(fresh));
    for (chained, fresh) in chained.parts.iter().zip(&fresh.parts) {
        if let (Some(chained), Some(fresh)) = (chained.nodes, fresh.nodes) {
            assert_eq!(chained, fresh, "part nodes");
        }
    }
}

/// Statistics of one differential run.
#[derive(Debug, Default)]
struct Tally {
    incremental: usize,
    whole_valid: usize,
    rejected: usize,
    /// Admitted patches that removed bindings.
    removed: usize,
}

/// Applies `patch` to `current` both ways and asserts identical results; returns the next
/// surface and index when admitted.
fn step(
    current: &Surface,
    index: &HudIndex,
    patch: &str,
    expect_incremental: bool,
    tally: &mut Tally,
) -> Option<(Surface, HudIndex)> {
    let whole = SurfacePatch::parse(patch).and_then(|p| p.apply(current, SurfaceRole::Hud));
    let indexed = SurfacePatch::parse(patch).and_then(|p| p.apply_hud(current, index));
    assert_eq!(
        indexed.as_ref().map(|(surface, _)| surface),
        whole.as_ref(),
        "indexed and whole admission differ for {patch}"
    );
    // The index's own verdict, before any whole-document fallback, is sound: whatever it
    // admits, whole-document validation admits, with the same document text.
    let Ok(parsed) = SurfacePatch::parse(patch) else {
        tally.rejected += 1;
        return None;
    };
    if let Some(controls) = &parsed.controls {
        let document = definitions::patch(&current.document, controls);
        let reference = document.clone().and_then(|document| {
            Surface {
                document,
                ..current.clone()
            }
            .validate_as(SurfaceRole::Hud)
        });
        match index.patched(current, controls) {
            Some((text, next)) => {
                assert!(
                    reference.is_ok(),
                    "index admitted what validation rejects: {patch}"
                );
                assert_eq!(Ok(&text), document.as_ref(), "patched document text");
                assert!(next.describes(&Surface {
                    document: text,
                    ..current.clone()
                }));
                tally.incremental += 1;
            }
            None if reference.is_ok() => {
                assert!(
                    !expect_incremental || index.0.is_none(),
                    "an indexed document fell back for {patch}"
                );
                tally.whole_valid += 1;
            }
            None => {}
        }
    }
    match indexed {
        Ok((surface, next)) => Some((surface, next)),
        Err(_) => {
            tally.rejected += 1;
            None
        }
    }
}

#[test]
fn random_control_patch_sequences_admit_exactly_what_whole_validation_admits() {
    let mut tally = Tally::default();
    for seed in 0..10 {
        let mut rng = Rng(seed);
        let mut current = representative();
        let mut index = HudIndex::new(&current).unwrap();
        assert!(index.describes(&current));
        for serial in 0..200 {
            if serial % 70 == 69 {
                current = representative();
                index = HudIndex::new(&current).unwrap();
            }
            let document: Value = serde_json::from_str(&current.document).unwrap();
            let bound: Vec<String> = current.bindings.keys().cloned().collect();
            let generated = generate(&mut rng, &document, &bound, serial);
            let patch = generated.patch.to_string();
            if let Some((next, next_index)) =
                step(&current, &index, &patch, generated.incremental, &mut tally)
            {
                if next.bindings.len() < current.bindings.len() {
                    tally.removed += 1;
                }
                current = next;
                index = next_index;
                if serial % 9 == 0 {
                    assert_consistent(&index, &current);
                }
            }
        }
    }
    // Every class of outcome was exercised.
    assert!(tally.incremental > 600, "{tally:?}");
    assert!(tally.whole_valid > 20, "{tally:?}");
    assert!(tally.rejected > 200, "{tally:?}");
    assert!(tally.removed > 50, "{tally:?}");
}

#[test]
fn drags_and_card_edits_on_a_representative_document_stay_incremental() {
    let mut current = representative();
    let mut index = HudIndex::new(&current).unwrap();
    let mut tally = Tally::default();
    let document: Value = serde_json::from_str(&current.document).unwrap();
    for round in 0..40 {
        let patch = if round % 2 == 0 {
            let mut root = document["root"].clone();
            root["controls"][2] = instance(2, 10 + round, 30 + round);
            json!({"controls":{"root":root}})
        } else {
            json!({"controls":{"c3":card(3, round as f64 / 40.)}})
        };
        (current, index) = step(&current, &index, &patch.to_string(), true, &mut tally).unwrap();
    }
    assert_eq!(tally.incremental, 40, "{tally:?}");
    assert_consistent(&index, &current);
}

#[test]
fn rejected_control_patches_fall_back_with_the_whole_document_error() {
    let current = representative();
    let index = HudIndex::new(&current).unwrap();
    let cases = [
        // A cycle through two cards.
        json!({"c0":{"type":"panel","controls":[{"x@hud.c1":{}}]},
            "c1":{"type":"panel","controls":[{"y@hud.c0":{}}]}}),
        // A base that reads its own definition.
        json!({"c2@hud.c2":{"type":"panel"}}),
        // A removed definition something still reads.
        json!({"bg":null}),
        // An unsupported renderer.
        json!({"c3":{"type":"custom","renderer":"unknown"}}),
        // Expanded property bytes past the bound.
        json!({"big":{"type":"label","$p|default":"y".repeat(1000),
            "text":format!("z{}", "$p".repeat(800))},
            "root":{"type":"panel","size":["100%","100%"],"controls":
                (0..4).map(|i| json!({format!("b{i}@hud.big"):{}})).collect::<Vec<_>>()}}),
        // The same root body inheriting a base that hides the root.
        json!({"rootbase":{"ignored":true},"root@hud.rootbase":
            serde_json::from_str::<Value>(&current.document).unwrap()["root"].clone()}),
        // A root whose own properties no longer draw.
        json!({"root":{"type":"custom","renderer":"unknown","controls":[{"w":{"type":"panel"}}]}}),
        // A base change that makes a card a label with children it cannot have.
        json!({"c4@hud.lb":{"controls":[{"x":{"type":"custom","renderer":"unknown"}}]}}),
    ];
    for controls in cases {
        let patch = json!({"controls":controls}).to_string();
        let parsed = SurfacePatch::parse(&patch).unwrap();
        assert!(
            index
                .patched(&current, parsed.controls.as_ref().unwrap())
                .is_none(),
            "{patch}"
        );
        let whole = SurfacePatch::parse(&patch)
            .unwrap()
            .apply(&current, SurfaceRole::Hud);
        let indexed = SurfacePatch::parse(&patch)
            .unwrap()
            .apply_hud(&current, &index);
        assert!(whole.is_err(), "{patch}");
        assert_eq!(indexed.map(|(surface, _)| surface), whole);
    }
}

#[test]
fn binding_patches_keep_the_index_and_stale_indexes_validate_whole() {
    let current = representative();
    let index = HudIndex::new(&current).unwrap();
    let (next, kept) = SurfacePatch::parse(r##"{"binding_changes":{"#v0":"9"}}"##)
        .unwrap()
        .apply_hud(&current, &index)
        .unwrap();
    assert!(kept.describes(&next), "bindings leave the document indexed");
    let (removed, kept) = SurfacePatch::parse(r##"{"remove_bindings":["#v1"]}"##)
        .unwrap()
        .apply_hud(&next, &kept)
        .unwrap();
    assert!(!removed.bindings.contains_key("#v1"));
    assert!(
        kept.describes(&removed),
        "removals leave the document indexed"
    );
    let mut tally = Tally::default();
    let patch = json!({"controls":{"c1":card(1, 0.5)},"remove_bindings":["#v2"],
        "binding_changes":{"#v1":"1"}});
    let (redefined, _) = step(&removed, &kept, &patch.to_string(), true, &mut tally).unwrap();
    assert_eq!(tally.incremental, 1, "{tally:?}");
    assert!(!redefined.bindings.contains_key("#v2"));
    assert!(redefined.bindings.contains_key("#v1"));
    let patch = json!({"controls":{"c1":card(1, 0.5)}}).to_string();
    let stale = HudIndex::default();
    let (patched, rebuilt) = SurfacePatch::parse(&patch)
        .unwrap()
        .apply_hud(&next, &stale)
        .unwrap();
    assert_eq!(
        patched,
        SurfacePatch::parse(&patch)
            .unwrap()
            .apply(&next, SurfaceRole::Hud)
            .unwrap()
    );
    assert!(
        rebuilt.describes(&patched),
        "whole validation indexes its result"
    );
}

/// A surface on the representative document with `extra` members added.
fn extended(extra: Value) -> Surface {
    let mut document: Value = serde_json::from_str(&representative().document).unwrap();
    for (key, body) in extra.as_object().unwrap() {
        document[key] = body.clone();
    }
    Surface {
        document: document.to_string(),
        ..representative()
    }
}

/// A panel definition of `nodes` authored nodes: itself and `nodes - 1` leaf children.
fn panel_of(nodes: usize) -> Value {
    let leaves: Vec<Value> = (1..nodes)
        .map(|leaf| json!({format!("n{leaf}"):{"type":"panel"}}))
        .collect();
    json!({"type":"panel","controls":leaves})
}

/// The index's own view of `index`.
fn inner(index: &HudIndex) -> &Index {
    index.0.as_deref().expect("indexed")
}

#[test]
fn authored_node_budget_admits_its_limit_and_rejects_one_more() {
    let current = extended(json!({"wide":panel_of(3500)}));
    let index = HudIndex::new(&current).unwrap();
    let room = MAX_SURFACE_NODES - inner(&index).authored_nodes;
    let mut tally = Tally::default();
    for (nodes, admitted) in [(room, true), (room + 1, false)] {
        let patch = json!({"controls":{"more":panel_of(nodes)}}).to_string();
        let next = step(&current, &index, &patch, true, &mut tally);
        assert_eq!(next.is_some(), admitted, "{nodes} more authored nodes");
    }
    assert_eq!(tally.incremental, 1, "{tally:?}");
}

#[test]
fn resolved_node_budget_is_never_reached_incrementally() {
    let block = panel_of(300);
    let entries: Vec<Value> = (0..12)
        .map(|copy| json!({format!("k{copy}@hud.block"):{}}))
        .collect();
    let mut document: Value = serde_json::from_str(&representative().document).unwrap();
    document["block"] = block;
    document["root"]["controls"]
        .as_array_mut()
        .unwrap()
        .extend(entries);
    let current = Surface {
        document: document.to_string(),
        ..representative()
    };
    let index = HudIndex::new(&current).unwrap();
    let room = MAX_SURFACE_NODES - inner(&index).resolved_nodes;
    let mut tally = Tally::default();
    // An inline root child of `nodes` resolved nodes: the last below the budget is admitted
    // incrementally, the budget itself only by whole validation, and beyond it neither.
    for (nodes, incremental) in [(room - 1, true), (room, false), (room + 1, false)] {
        let mut root = document["root"].clone();
        root["controls"]
            .as_array_mut()
            .unwrap()
            .push(json!({"inline":panel_of(nodes)}));
        let patch = json!({"controls":{"root":root}}).to_string();
        let before = tally.incremental;
        let next = step(&current, &index, &patch, incremental, &mut tally);
        assert_eq!(
            tally.incremental > before,
            incremental,
            "{nodes} more nodes"
        );
        if nodes > room {
            assert!(next.is_none(), "past the node budget");
        }
    }
}

#[test]
fn document_byte_limit_admits_its_limit_and_rejects_one_byte_more() {
    let pad = |bytes: usize| json!({"type":"label","text":"x".repeat(bytes)});
    let current = extended(json!({"pad0":pad(150 * 1024)}));
    let index = HudIndex::new(&current).unwrap();
    // `,"pad1":` and the body around its text.
    let framing = 8 + pad(0).to_string().len();
    let room = MAX_SURFACE_BYTES - current.document.len() - framing;
    let mut tally = Tally::default();
    for (bytes, admitted) in [(room, true), (room + 1, false)] {
        let patch = json!({"controls":{"pad1":pad(bytes)}}).to_string();
        let next = step(&current, &index, &patch, true, &mut tally);
        assert_eq!(next.is_some(), admitted, "{bytes} text bytes");
        if let Some((next, _)) = next {
            assert_eq!(next.document.len(), MAX_SURFACE_BYTES);
        }
    }
    assert_eq!(tally.incremental, 1, "{tally:?}");
}
