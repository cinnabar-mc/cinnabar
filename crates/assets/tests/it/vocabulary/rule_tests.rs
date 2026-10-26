use super::*;

/// Builds one missing actor occurrence beside a valid member and an unrelated rule.
fn fixture() -> (Vocabulary, Value, Vec<Exception>) {
    let vocabulary = Vocabulary {
        domains: BTreeMap::from([(
            "actor".into(),
            BTreeSet::from(["known:actor".into(), "known:inserted".into()]),
        )]),
    };
    let document = serde_json::json!({"entity_rules": [
        {"id": "retained", "members": ["minecraft:vocabulary_missing_actor", "known:actor"]},
        {"id": "other", "members": ["known:actor"]}
    ]});
    let exceptions = collect(&document)
        .into_iter()
        .filter(|reference| !vocabulary.contains(reference))
        .map(|reference| Exception {
            reference,
            reason: "Retained compatibility member.".into(),
        })
        .collect();
    (vocabulary, document, exceptions)
}

/// Collects policy occurrences through the same walker as the committed-data gate.
fn collect(document: &Value) -> BTreeSet<Reference> {
    let mut refs = BTreeSet::new();
    walk(&mut refs, "registry-policy.json", document, "", "block");
    refs
}

#[test]
fn rule_insertion_and_reordering_preserve_exceptions() {
    let (vocabulary, mut document, exceptions) = fixture();
    assert!(failures(&vocabulary, &collect(&document), &exceptions).is_empty());
    let rules = document["entity_rules"].as_array_mut().unwrap();
    rules.insert(0, serde_json::json!({"id": "inserted", "members": []}));
    let errors = failures(&vocabulary, &collect(&document), &exceptions);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    document["entity_rules"].as_array_mut().unwrap().reverse();
    let errors = failures(&vocabulary, &collect(&document), &exceptions);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn member_insertion_and_reordering_preserve_exceptions() {
    let (vocabulary, mut document, exceptions) = fixture();
    let members = document["entity_rules"][0]["members"]
        .as_array_mut()
        .unwrap();
    members.insert(0, Value::String("known:inserted".into()));
    let errors = failures(&vocabulary, &collect(&document), &exceptions);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    document["entity_rules"][0]["members"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let errors = failures(&vocabulary, &collect(&document), &exceptions);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn another_rule_collection_or_field_does_not_share_an_exception() {
    let (vocabulary, document, exceptions) = fixture();
    for (collection, id, field) in [
        ("entity_rules", "other", "members"),
        ("block_rules", "retained", "members"),
        ("entity_rules", "retained", "exclude_members"),
    ] {
        let mut changed = document.clone();
        let rules = changed[collection].as_array_mut();
        if let Some(rules) = rules {
            let rule = rules.iter_mut().find(|rule| rule["id"] == id).unwrap();
            rule[field] = serde_json::json!(["minecraft:vocabulary_missing_actor"]);
        } else {
            changed[collection] = serde_json::json!([
                {"id": id, field: ["minecraft:vocabulary_missing_actor"]}
            ]);
        }
        let errors = failures(&vocabulary, &collect(&changed), &exceptions);
        assert_eq!(errors.len(), 1, "{collection}/{id}/{field}: {errors:?}");
        assert!(errors[0].contains("unknown"), "{errors:?}");
        assert!(errors[0].contains(id), "{errors:?}");
        assert!(errors[0].contains(field), "{errors:?}");
    }
}

#[test]
fn removing_or_renaming_a_rule_or_member_leaves_a_stale_exception() {
    let (vocabulary, document, exceptions) = fixture();
    for mutation in [
        "remove_rule",
        "rename_rule",
        "remove_member",
        "rename_member",
    ] {
        let mut changed = document.clone();
        match mutation {
            "remove_rule" => {
                changed["entity_rules"].as_array_mut().unwrap().remove(0);
            }
            "rename_rule" => changed["entity_rules"][0]["id"] = "renamed".into(),
            "remove_member" => {
                changed["entity_rules"][0]["members"]
                    .as_array_mut()
                    .unwrap()
                    .remove(0);
            }
            "rename_member" => {
                changed["entity_rules"][0]["members"][0] =
                    "minecraft:vocabulary_renamed_actor".into()
            }
            _ => unreachable!(),
        }
        let errors = failures(&vocabulary, &collect(&changed), &exceptions);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("stale exception") && error.contains("retained")),
            "{mutation}: {errors:?}"
        );
        assert_eq!(
            errors.len(),
            if mutation.starts_with("rename") { 2 } else { 1 },
            "{mutation}: {errors:?}"
        );
    }
}

#[test]
fn unknown_stable_references_and_positional_fallbacks_are_rejected() {
    let (vocabulary, document, exceptions) = fixture();
    for location in [
        Location::Rule {
            collection: "unknown_rules".into(),
            id: "retained".into(),
            field: "/members".into(),
        },
        Location::Rule {
            collection: "entity_rules".into(),
            id: "unknown".into(),
            field: "/members".into(),
        },
        Location::Rule {
            collection: "entity_rules".into(),
            id: "retained".into(),
            field: "/unknown".into(),
        },
        Location::Pointer("/entity_rules/0/members/0".into()),
    ] {
        let mut changed = exceptions.clone();
        changed[0].reference.location = location;
        let errors = failures(&vocabulary, &collect(&document), &changed);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains("stale exception"), "{errors:?}");
        assert!(errors[1].contains("unknown"), "{errors:?}");
    }
}

#[test]
fn stable_exceptions_keep_file_domain_duplicate_reason_and_unused_checks() {
    let (mut vocabulary, document, exceptions) = fixture();
    let refs = collect(&document);
    for field in ["file", "domain"] {
        let mut changed = exceptions.clone();
        match field {
            "file" => changed[0].reference.file = "other.json".into(),
            "domain" => changed[0].reference.domain = "block".into(),
            _ => unreachable!(),
        }
        assert_eq!(failures(&vocabulary, &refs, &changed).len(), 2);
    }
    let duplicated = [exceptions[0].clone(), exceptions[0].clone()];
    let errors = failures(&vocabulary, &refs, &duplicated);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("duplicate exception"), "{errors:?}");
    let mut empty_reason = exceptions.clone();
    empty_reason[0].reason = "  ".into();
    let errors = failures(&vocabulary, &refs, &empty_reason);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(
        errors[0].contains("empty or duplicate exception"),
        "{errors:?}"
    );
    assert!(errors[1].contains("unknown"), "{errors:?}");
    vocabulary
        .domains
        .get_mut("actor")
        .unwrap()
        .insert(exceptions[0].reference.identifier.clone());
    let errors = failures(&vocabulary, &refs, &exceptions);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("stale exception"), "{errors:?}");
}

#[test]
#[should_panic(expected = "duplicate rule ID \"retained\" in registry-policy.json/entity_rules")]
fn duplicate_rule_ids_are_rejected() {
    let (_, mut document, _) = fixture();
    document["entity_rules"][1]["id"] = "retained".into();
    collect(&document);
}

#[test]
#[should_panic(expected = "missing stable rule ID in registry-policy.json/entity_rules/0")]
fn missing_rule_ids_are_rejected() {
    let (_, mut document, _) = fixture();
    document["entity_rules"][0]
        .as_object_mut()
        .unwrap()
        .remove("id");
    collect(&document);
}

#[test]
#[should_panic(expected = "missing stable rule ID in registry-policy.json/entity_rules/0")]
fn empty_rule_ids_are_rejected() {
    let (_, mut document, _) = fixture();
    document["entity_rules"][0]["id"] = "  ".into();
    collect(&document);
}

#[test]
#[should_panic(expected = "ambiguous rule identifier occurrence")]
fn duplicate_identifier_occurrences_are_rejected() {
    let (_, mut document, _) = fixture();
    let duplicate = document["entity_rules"][0]["members"][0].clone();
    document["entity_rules"][0]["members"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    collect(&document);
}

#[test]
fn locations_reject_ambiguous_unknown_and_incomplete_schemas() {
    for location in [
        serde_json::json!({}),
        serde_json::json!({"pointer": "/old", "rule": {"collection": "entity_rules", "id": "retained", "field": "/members"}}),
        serde_json::json!({"unknown": "/members"}),
        serde_json::json!({"rule": {"id": "retained", "field": "/members"}}),
        serde_json::json!({"rule": {"collection": "entity_rules", "id": "retained", "field": "/members", "index": 0}}),
    ] {
        assert!(
            serde_json::from_value::<Location>(location.clone()).is_err(),
            "{location}"
        );
    }
}

#[test]
fn exception_documents_reject_unsupported_and_malformed_schemas() {
    let original = json(&root().join("crates/assets/data/vocabulary-exceptions.json"));
    assert!(Exceptions::parse(&serde_json::to_vec(&original).unwrap()).is_ok());
    for mutation in ["old_schema", "unknown_field", "missing_location"] {
        let mut changed = original.clone();
        match mutation {
            "old_schema" => changed["schema"] = 1.into(),
            "unknown_field" => changed["unknown"] = true.into(),
            "missing_location" => {
                changed["entries"][0]["reference"]
                    .as_object_mut()
                    .unwrap()
                    .remove("location");
            }
            _ => unreachable!(),
        }
        assert!(
            Exceptions::parse(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn committed_rule_exceptions_survive_reordering_every_named_collection() {
    let vocabulary = Vocabulary::load(&root());
    let mut document = json(&root().join("crates/asset-compiler/data/registry-policy.json"));
    let exceptions = Exceptions::parse(
        &fs::read(root().join("crates/assets/data/vocabulary-exceptions.json")).unwrap(),
    )
    .unwrap();
    let exceptions: Vec<_> = exceptions
        .entries
        .into_iter()
        .filter(|exception| matches!(exception.reference.location, Location::Rule { .. }))
        .collect();
    let before: BTreeSet<_> = collect(&document)
        .into_iter()
        .filter(|reference| matches!(reference.location, Location::Rule { .. }))
        .collect();
    assert!(failures(&vocabulary, &before, &exceptions).is_empty());
    for rules in document
        .as_object_mut()
        .unwrap()
        .values_mut()
        .filter_map(Value::as_array_mut)
    {
        if !rules.iter().any(|rule| rule.get("id").is_some()) {
            continue;
        }
        rules.reverse();
        for rule in rules.iter_mut() {
            reverse_string_lists(rule);
        }
        rules.insert(
            0,
            serde_json::json!({"id": "inserted-vocabulary-test-rule", "members": []}),
        );
    }
    let after: BTreeSet<_> = collect(&document)
        .into_iter()
        .filter(|reference| matches!(reference.location, Location::Rule { .. }))
        .collect();
    assert_eq!(before, after);
    let errors = failures(&vocabulary, &after, &exceptions);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// Reorders identifier lists recursively without changing scalar fields or rule semantics.
fn reverse_string_lists(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for value in fields.values_mut() {
                reverse_string_lists(value);
            }
        }
        Value::Array(values) if values.iter().all(Value::is_string) => values.reverse(),
        _ => {}
    }
}
