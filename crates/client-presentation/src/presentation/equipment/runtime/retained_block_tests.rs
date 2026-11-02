//! Stacks whose wire descriptor names a block but whose aux value has no compiled item visual.

use super::*;

/// A visual the fixture prepared no art for.
const UNPREPARED: u32 = 3;

fn retained(state: Option<u32>, item: Option<u32>) -> HeldKind {
    HeldKind::RetainedBlock { state, item }
}

#[test]
fn retained_stack_holds_the_state_it_names() {
    let (mut runtime, body, mut item) = block_fixture();
    let HeldKind::Block(visual) = item.kind else {
        unreachable!()
    };
    item.kind = retained(Some(visual), Some(UNPREPARED));
    assert_eq!(runtime.held_block_visual(item.kind), Some(visual));
    third_person(&mut runtime, &body, &item);
    let (_, _, block, _) = runtime.held_mesh(&item, true).unwrap();
    assert!(block, "it holds like a block, not a flat sprite");
}

#[test]
fn retained_state_without_art_holds_its_items_block() {
    let (mut runtime, _, mut item) = block_fixture();
    let HeldKind::Block(visual) = item.kind else {
        unreachable!()
    };
    item.kind = retained(Some(UNPREPARED), Some(visual));
    assert_eq!(runtime.held_block_visual(item.kind), Some(visual));
    let (_, _, block, _) = runtime.held_mesh(&item, true).unwrap();
    assert!(block);
}

#[test]
fn retained_stack_with_no_block_art_keeps_the_block_refusals() {
    let (mut runtime, _, mut item) = block_fixture();
    item.kind = retained(Some(UNPREPARED), None);
    assert!(runtime.held_mesh(&item, false).is_none());
    item.kind = retained(None, None);
    assert!(runtime.held_mesh(&item, false).is_none());
}

/// The installed carriers, or `None` after naming the missing fixture.
fn installed_runtime(test: &str) -> Option<(EquipmentRuntime, Arc<RuntimeEntityAssets>)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let compiled = root.join(".local/assets/compiled");
    let read = |path: std::path::PathBuf| match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            eprintln!(
                "skipping {test}: missing carrier fixture {} (make assets)",
                path.display()
            );
            None
        }
    };
    let world = read(root.join(assets::carriers::startup::DEFAULT_ASSET_PATH))?;
    let entities = read(compiled.join("vanilla-v1.mcbeent"))?;
    let icons = read(compiled.join("vanilla-v1.mcbeico"))?;
    let world = Arc::new(RuntimeAssets::decode(&world).unwrap());
    let entities = Arc::new(RuntimeEntityAssets::decode(&entities).unwrap());
    let icons = Arc::new(RuntimeIconCatalog::decode(&icons).unwrap());
    let (runtime, _, _) = EquipmentRuntime::build(
        Arc::clone(&entities),
        None,
        icons,
        Some(world),
        None,
        ActorArtworkPages::default(),
    );
    Some((runtime, entities))
}

/// Every block item in the installed carriers holds the same whatever aux value and block state
/// its stack carries. This is the coverage of servers that send legacy aux values, read from the
/// carriers and the pinned block registry rather than from a list of names.
#[test]
fn every_installed_block_item_holds_with_a_retained_state() {
    let test = "every_installed_block_item_holds_with_a_retained_state";
    let Some((mut runtime, entities)) = installed_runtime(test) else {
        return;
    };
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let (_, _, template) = block_fixture();
    let mut blocks_by_item = BTreeMap::<&str, u32>::new();
    let mut lost = Vec::new();
    let mut checked = 0_usize;
    // Whether the stack draws anything, and whether it draws as a block.
    let mut holds = |identifier: &str, metadata, kind| {
        let mut item = template.clone();
        item.identifier = Arc::from(identifier);
        item.metadata = metadata;
        item.kind = kind;
        // The mesh table is bounded for one session's items; this walks the whole catalog.
        runtime.meshes.clear();
        runtime.pending.clear();
        runtime.next_mesh = 0;
        runtime.held_mesh(&item, true).map(|(_, _, block, _)| block)
    };
    for definition in entities.item_visuals() {
        let assets::ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            continue;
        };
        let identifier = definition.key.identifier.as_ref();
        let plain = holds(
            identifier,
            definition.key.metadata,
            HeldKind::Block(block_visual.0),
        );
        if plain.is_none() {
            continue;
        }
        checked += 1;
        if definition.key.metadata == 0 && plain == Some(true) {
            blocks_by_item.insert(identifier, block_visual.0);
        }
        let kind = retained(Some(block_visual.0), Some(block_visual.0));
        if holds(identifier, 15, kind) != plain {
            lost.push(identifier.to_owned());
        }
    }
    assert!(checked > 500, "only {checked} block items draw");
    assert!(
        blocks_by_item.len() > 300,
        "only {} hold as blocks",
        blocks_by_item.len()
    );
    for record in records.iter() {
        let Some(&base) = blocks_by_item.get(record.name.as_ref()) else {
            continue;
        };
        checked += 1;
        let kind = retained(Some(record.sequential_id), Some(base));
        if holds(&record.name, 15, kind) != Some(true) {
            lost.push(format!("{} state {}", record.name, record.canonical_state));
        }
    }
    lost.sort();
    lost.dedup();
    eprintln!("checked {checked} stacks, {} lost their art", lost.len());
    assert!(lost.is_empty(), "lost their held art: {lost:#?}");
}
