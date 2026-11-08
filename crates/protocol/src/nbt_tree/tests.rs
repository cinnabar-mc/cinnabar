use super::test_tree::Nbt as Tree;
use super::*;

#[test]
fn review_nbt_varints_reject_unused_high_bits() {
    let bytes = [10, 0, 9, 1, 120, 1, 128, 128, 128, 128, 16, 0];
    assert!(read_root(&bytes).is_none());
    let mut reader = Cursor {
        bytes: &[128, 128, 128, 128, 128, 128, 128, 128, 128, 2],
        position: 0,
    };
    assert!(reader.var_u64(10).is_none());
    let mut valid = Cursor {
        bytes: &[255, 255, 255, 255, 255, 255, 255, 255, 255, 1],
        position: 0,
    };
    assert_eq!(valid.var_u64(10), Some(u64::MAX));
}

/// Lookups past nested containers land on the right sibling, by span and by walk.
#[test]
fn lookups_step_over_nested_fields() {
    let leaf = |value| Tree::Compound(vec![("inner".into(), Tree::List(vec![Tree::Int(value)]))]);
    let nested = Tree::Compound(vec![
        (
            "list".into(),
            Tree::List(vec![leaf(-3), Tree::Compound(vec![])]),
        ),
        (
            "lists".into(),
            Tree::List(vec![
                Tree::List(vec![leaf(4), leaf(5)]),
                Tree::List(vec![leaf(6)]),
            ]),
        ),
        (
            "deep".into(),
            Tree::Compound(vec![("x".into(), Tree::Byte(1))]),
        ),
        ("name".into(), Tree::String("value".into())),
        ("after".into(), Tree::Long(-9)),
    ]);
    let bytes = nested.encode_wrapped();
    let document = read_root(&bytes).unwrap();
    assert_eq!(
        document.spans.len(),
        8,
        "the wrapper, list, lists, deep and four inner lists"
    );
    let walked = NbtDocument {
        spans: Vec::new(),
        indexed: false,
        ..read_root(&bytes).unwrap()
    };
    assert_eq!(
        format!("{:?}", document.root()),
        format!("{:?}", walked.root()),
        "spans and walks decode the same tree"
    );
    for root in [document.root(), walked.root()] {
        let root = root.field("").unwrap();
        assert_eq!(root.field("name").and_then(Nbt::as_str), Some("value"));
        assert!(matches!(root.field("after"), Some(Nbt::Long(-9))));
        assert!(matches!(
            root.field("deep").and_then(|deep| deep.field("x")),
            Some(Nbt::Byte(1))
        ));
        let list = root.list("list");
        assert_eq!(list.len(), 2);
        let inner = list.iter().next().unwrap().list("inner");
        assert!(matches!(inner.iter().next(), Some(Nbt::Int(-3))));
        assert!(matches!(list.iter().nth(1), Some(Nbt::Compound(compound)) if compound.is_empty()));
        assert!(list.iter().nth(2).is_none());
        let inner: Vec<_> = root
            .list("lists")
            .iter()
            .flat_map(|list| match list {
                Nbt::List(list) => list.iter().collect(),
                _ => Vec::new(),
            })
            .map(|leaf| leaf.list("inner").iter().next().and_then(Nbt::int32))
            .collect();
        assert_eq!(inner, [Some(4), Some(5), Some(6)]);
    }
}

#[test]
fn values_decode_and_duplicates_are_seen() {
    let value = Tree::Compound(vec![
        ("a".into(), Tree::Short(-2)),
        ("a".into(), Tree::Float(0.5)),
        (
            "floats".into(),
            Tree::List(vec![Tree::Float(1.0), Tree::Float(2.5)]),
        ),
        ("other".into(), Tree::Other),
    ])
    .view();
    assert!(
        matches!(value.field("a"), Some(Nbt::Short(-2))),
        "first wins"
    );
    assert!(value.duplicate_field("a"));
    assert!(!value.duplicate_field("floats"));
    let floats: Vec<_> = value
        .list("floats")
        .iter()
        .filter_map(Nbt::number)
        .collect();
    assert_eq!(floats, [1.0, 2.5]);
    assert_eq!(
        value.list("floats").iter().nth(1).and_then(Nbt::number),
        Some(2.5)
    );
    assert!(matches!(value.field("other"), Some(Nbt::Other)));
    assert!(value.list("a").is_empty());
    let names: Vec<_> = match value {
        Nbt::Compound(compound) => compound.iter().map(|(name, _)| name).collect(),
        _ => unreachable!(),
    };
    assert_eq!(names, ["a", "a", "floats", "other"]);
}

#[test]
fn malformed_payloads_are_rejected() {
    // Truncated, unknown tag, invalid UTF-8 and a list one level too deep.
    assert!(read_root(&[10, 0, 1, 1, b'a']).is_none());
    assert!(read_root(&[10, 0, 13, 0, 0]).is_none());
    assert!(read_root(&[10, 0, 8, 1, b'a', 1, 0xff, 0]).is_none());
    // The wrapping field is a list at depth 1, so `levels` lists put the byte at
    // depth `levels + 1`.
    let nested = |levels: usize| {
        let mut value = Tree::Byte(1);
        for _ in 0..levels {
            value = Tree::List(vec![value]);
        }
        value.encode_wrapped()
    };
    assert!(read_root(&nested(MAX_NBT_DEPTH - 1)).is_some());
    assert!(read_root(&nested(MAX_NBT_DEPTH)).is_none());
}

/// Each shape a per-element tree expands most allocates only its spans.
#[test]
fn decoding_allocates_at_most_eight_bytes_per_three_wire_bytes() {
    for (shape, field) in test_tree::amplifying_fields("", 1 << 16) {
        let mut bytes = vec![10, 0];
        bytes.extend(&field);
        bytes.push(0);
        let (document, allocations) = test_alloc::measure(|| read_root(&bytes));
        let document = document.unwrap();
        assert!(
            allocations.bytes * 3 <= bytes.len() * 8,
            "{shape}: {} bytes for {}",
            allocations.bytes,
            bytes.len()
        );
        let (count, allocations) = test_alloc::measure(|| match document.root().field("") {
            Some(Nbt::List(list)) => list.iter().count(),
            Some(Nbt::Compound(compound)) => compound.iter().count(),
            _ => 0,
        });
        assert!(count > 0, "{shape}");
        assert_eq!(allocations.bytes, 0, "{shape}: views allocate nothing");
    }
}
