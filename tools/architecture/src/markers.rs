use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use proc_macro2::{TokenStream, TokenTree};

use crate::{
    ArchitectureError,
    paths::{marker_literals, relative_slash},
    policy::{MarkerKind, MarkerRule, Policy},
    read,
};

pub(super) fn check_markers(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    // The marker prefix is shared with ordinary environment variables, so undeclared literals
    // are reported only from the declared producers and the acceptance scripts.
    let producers = policy
        .markers
        .iter()
        .map(|rule| rule.producer.as_str())
        .collect::<BTreeSet<_>>();
    let consumers = policy
        .markers
        .iter()
        .filter_map(|rule| rule.consumer.as_deref())
        .collect::<BTreeSet<_>>();
    let mut occurrences = BTreeMap::<String, Vec<String>>::new();
    let mut registered = BTreeSet::<String>::new();
    for path in files {
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("rs" | "ps1")
        ) {
            continue;
        }
        let relative = relative_slash(root, path);
        let registry = producers.contains(relative.as_str())
            || relative == "scripts/acceptance.ps1"
            || relative.starts_with("scripts/acceptance/");
        if !registry && !consumers.contains(relative.as_str()) {
            continue;
        }
        for marker in marker_literals(&read(path)?) {
            if registry {
                registered.insert(marker.clone());
            }
            occurrences
                .entry(marker)
                .or_default()
                .push(relative.clone());
        }
    }
    let declared = policy
        .markers
        .iter()
        .map(|rule| rule.literal.as_str())
        .collect::<BTreeSet<_>>();
    for marker in &registered {
        if !declared.contains(marker.as_str()) {
            diagnostics.push(format!("marker `{marker}` has no declared expectation"));
        }
    }
    for rule in &policy.markers {
        let entries = occurrences.get(&rule.literal).cloned().unwrap_or_default();
        let producer_count = entries
            .iter()
            .filter(|path| *path == &rule.producer)
            .count();
        if producer_count != 1 {
            diagnostics.push(format!(
                "marker `{}` producer count {producer_count}, expected 1 in {}",
                rule.literal, rule.producer
            ));
        }
        if rule.kind == MarkerKind::Parsed {
            let consumer_count = rule
                .consumer
                .as_ref()
                .map(|consumer| entries.iter().filter(|path| *path == consumer).count())
                .unwrap_or(0);
            if consumer_count == 0 && !symbolic_consumer(root, rule)? {
                diagnostics.push(format!(
                    "marker `{}` has no occurrence in declared consumer {}",
                    rule.literal,
                    rule.consumer
                        .as_deref()
                        .unwrap_or("<missing policy consumer>")
                ));
            }
        } else if rule.kind == MarkerKind::EnvironmentVariable {
            let consumer = rule.consumer.as_deref();
            if !symbolic_consumer(root, rule)? {
                diagnostics.push(format!(
                    "environment variable marker `{}` has no symbolic use in declared consumer {}",
                    rule.literal,
                    consumer.unwrap_or("<missing policy consumer>")
                ));
            }
        }
    }
    Ok(())
}

/// Checks the declared consumer against the producer's actual constant name.
fn symbolic_consumer(root: &Path, rule: &MarkerRule) -> Result<bool, ArchitectureError> {
    let producer = read(&root.join(&rule.producer))?;
    let consumed = marker_symbol(&producer, &rule.literal)
        .zip(rule.consumer.as_deref())
        .is_some_and(|(symbol, consumer)| {
            read(&root.join(consumer)).is_ok_and(|source| {
                source
                    .parse::<TokenStream>()
                    .is_ok_and(|tokens| symbolic_path(tokens, symbol))
            })
        });
    Ok(consumed)
}

/// Recognizes qualified paths inside Rust and macros without counting comments or string contents.
fn symbolic_path(tokens: TokenStream, symbol: &str) -> bool {
    let mut colons = 0;
    for token in tokens {
        match token {
            TokenTree::Group(group) => {
                if symbolic_path(group.stream(), symbol) {
                    return true;
                }
                colons = 0;
            }
            TokenTree::Punct(punctuation) if punctuation.as_char() == ':' => colons += 1,
            TokenTree::Ident(identifier) if colons == 2 && identifier == symbol => return true,
            _ => colons = 0,
        }
    }
    false
}

/// Finds the constant declaration that owns the exact marker literal.
fn marker_symbol<'a>(source: &'a str, literal: &str) -> Option<&'a str> {
    source.lines().find_map(|line| {
        if !line.contains(&format!("\"{literal}\"")) {
            return None;
        }
        let declaration = line.split_once("const ")?.1;
        declaration.split_once(':').map(|(name, _)| name.trim())
    })
}
