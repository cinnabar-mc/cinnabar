use super::*;
use crate::protocol::Mining;

fn block(name: &str) -> BlockDef {
    BlockDef {
        id: format!("probe:{name}"),
        display_name: "Probe".to_owned(),
        textures: Vec::new(),
        mining: Mining::Unbreakable {},
        states: Vec::new(),
        placement: Vec::new(),
        visual: None,
        permutations: Vec::new(),
        network: false,
        drops: BlockDrops::Itself,
        item: BlockItem::Listed,
        redstone: false,
    }
}

fn item(name: &str) -> ItemDef {
    ItemDef {
        id: format!("probe:{name}"),
        display_name: "Probe Cell".to_owned(),
        icon: "/icon.png".to_owned(),
        max_stack: 1,
        listed: true,
    }
}

fn rule(name: &str, drops: wit::BlockDrops, item: wit::BlockItem) -> wit::BlockRule {
    wit::BlockRule {
        block: format!("probe:{name}"),
        drops,
        item,
        redstone: false,
    }
}

fn rules(blocks: Vec<wit::BlockRule>, unlisted: &[&str]) -> wit::ContentRules {
    wit::ContentRules {
        blocks,
        unlisted_items: unlisted
            .iter()
            .map(|name| format!("probe:{name}"))
            .collect(),
    }
}

/// Rules set the drops, item and redstone of the blocks they name and unlist the items they
/// name; the rest keep dropping themselves, stay listed and take no part in redstone.
#[test]
fn rules_set_drops_items_redstone_and_listing() {
    let (blocks, items) = apply(
        vec![
            block("bus"),
            block("counter"),
            block("part"),
            block("emitter"),
        ],
        vec![item("cell"), item("pattern")],
        rules(
            vec![
                rule("bus", wit::BlockDrops::Nothing, wit::BlockItem::None),
                rule("part", wit::BlockDrops::Itself, wit::BlockItem::Unlisted),
                wit::BlockRule {
                    redstone: true,
                    ..rule("emitter", wit::BlockDrops::Itself, wit::BlockItem::Listed)
                },
            ],
            &["pattern"],
        ),
    )
    .unwrap();
    let kinds: Vec<_> = blocks
        .iter()
        .map(|block| (block.drops, block.item, block.redstone))
        .collect();
    assert_eq!(
        kinds,
        [
            (BlockDrops::Nothing, BlockItem::None, false),
            (BlockDrops::Itself, BlockItem::Listed, false),
            (BlockDrops::Itself, BlockItem::Unlisted, false),
            (BlockDrops::Itself, BlockItem::Listed, true),
        ]
    );
    let listed: Vec<_> = items.iter().map(|item| item.listed).collect();
    assert_eq!(listed, [true, false]);
}

/// A rule for another block, a second rule for one block, a block without an item that drops
/// itself, and an unknown or repeated unlisted item each fail the load.
#[test]
fn bad_rules_fail() {
    let cases = [
        (
            rules(
                vec![rule(
                    "other",
                    wit::BlockDrops::Nothing,
                    wit::BlockItem::Listed,
                )],
                &[],
            ),
            "names none of the Experience's blocks",
        ),
        (
            rules(
                vec![
                    rule("bus", wit::BlockDrops::Nothing, wit::BlockItem::Listed),
                    rule("bus", wit::BlockDrops::Nothing, wit::BlockItem::None),
                ],
                &[],
            ),
            "has two rules",
        ),
        (
            rules(
                vec![rule("bus", wit::BlockDrops::Itself, wit::BlockItem::None)],
                &[],
            ),
            "cannot drop itself",
        ),
        (
            rules(Vec::new(), &["other"]),
            "is none of the Experience's items",
        ),
        (rules(Vec::new(), &["cell", "cell"]), "is unlisted twice"),
    ];
    for (rules, expected) in cases {
        let error = apply(vec![block("bus")], vec![item("cell")], rules).unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
    }
}
