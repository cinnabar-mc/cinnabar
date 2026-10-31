//! `minecraft:render_offsets`: how a server item's flat sprite sits in each hand.

use render_api::{DEFAULT_ITEM_RENDER_OFFSETS, HeldItemHand, HeldItemTransform, ItemRenderOffsets};

use crate::nbt_tree::Nbt;

/// Reads a component onto the default tables, as the client loads it into a default-constructed
/// component: fields a server leaves out or sends malformed keep their default. A string has
/// no effect on the tables but still counts as a declared component, since vanilla's named value
/// conversion writes nothing. Other values are not a component.
pub(super) fn parse_render_offsets(component: Option<&Nbt>) -> Option<ItemRenderOffsets> {
    let component = component?;
    let mut offsets = DEFAULT_ITEM_RENDER_OFFSETS;
    match component {
        Nbt::String(_) => {}
        Nbt::Compound(_) => {
            overlay_hand(&mut offsets.main_hand, component.field("main_hand"));
            overlay_hand(&mut offsets.off_hand, component.field("off_hand"));
        }
        _ => return None,
    }
    Some(offsets)
}

fn overlay_hand(hand: &mut HeldItemHand, declared: Option<&Nbt>) {
    let Some(declared) = declared else {
        return;
    };
    overlay_transform(&mut hand.first_person, declared.field("first_person"));
    overlay_transform(&mut hand.third_person, declared.field("third_person"));
}

fn overlay_transform(transform: &mut HeldItemTransform, declared: Option<&Nbt>) {
    let Some(declared) = declared else {
        return;
    };
    for (name, target) in [
        ("position", &mut transform.position),
        ("rotation", &mut transform.rotation),
        ("scale", &mut transform.scale),
    ] {
        if let Some(values) = finite_vector(declared.list(name)) {
            *target = values;
        }
    }
}

/// Exactly three numbers that stay finite as `f32`.
fn finite_vector(values: &[Nbt]) -> Option<[f32; 3]> {
    let [x, y, z] = values else {
        return None;
    };
    let number = |value: &Nbt| {
        value
            .number()
            .map(|value| value as f32)
            .filter(|value| value.is_finite())
    };
    Some([number(x)?, number(y)?, number(z)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(values: [f64; 3]) -> Nbt {
        Nbt::List(values.into_iter().map(Nbt::Float).collect())
    }

    fn compound<const N: usize>(fields: [(&str, Nbt); N]) -> Nbt {
        Nbt::Compound(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    #[test]
    fn an_absent_or_unsupported_value_is_not_a_component() {
        assert_eq!(parse_render_offsets(None), None);
        assert_eq!(parse_render_offsets(Some(&Nbt::Int(1))), None);
    }

    #[test]
    fn a_named_value_keeps_the_default_tables() {
        assert_eq!(
            parse_render_offsets(Some(&Nbt::String("tools".into()))),
            Some(DEFAULT_ITEM_RENDER_OFFSETS)
        );
    }

    #[test]
    fn declared_fields_overwrite_only_themselves() {
        let component = compound([(
            "main_hand",
            compound([(
                "first_person",
                compound([("scale", floats([0.1, 0.2, 0.3]))]),
            )]),
        )]);
        let mut expected = DEFAULT_ITEM_RENDER_OFFSETS;
        expected.main_hand.first_person.scale = [0.1, 0.2, 0.3];
        assert_eq!(parse_render_offsets(Some(&component)), Some(expected));
    }

    #[test]
    fn every_hand_and_perspective_reads_its_own_tables() {
        let transform = |base: f64| {
            compound([
                ("position", floats([base, base + 1.0, base + 2.0])),
                ("rotation", floats([base + 3.0, base + 4.0, base + 5.0])),
                ("scale", floats([base + 6.0, base + 7.0, base + 8.0])),
            ])
        };
        let hand = |base: f64| {
            compound([
                ("first_person", transform(base)),
                ("third_person", transform(base + 10.0)),
            ])
        };
        let component = compound([("main_hand", hand(0.0)), ("off_hand", hand(20.0))]);
        let offsets = parse_render_offsets(Some(&component)).unwrap();
        assert_eq!(offsets.main_hand.first_person.position, [0.0, 1.0, 2.0]);
        assert_eq!(offsets.main_hand.third_person.rotation, [13.0, 14.0, 15.0]);
        assert_eq!(offsets.off_hand.first_person.scale, [26.0, 27.0, 28.0]);
        assert_eq!(offsets.off_hand.third_person.position, [30.0, 31.0, 32.0]);
    }

    #[test]
    fn malformed_vectors_keep_the_default() {
        for malformed in [
            Nbt::List(vec![Nbt::Float(1.0), Nbt::Float(2.0)]),
            floats([f64::NAN, 0.0, 0.0]),
            floats([f64::MAX, 0.0, 0.0]),
            Nbt::List(vec![
                Nbt::Float(1.0),
                Nbt::String("x".into()),
                Nbt::Float(1.0),
            ]),
            Nbt::Float(1.0),
        ] {
            let component = compound([(
                "off_hand",
                compound([("third_person", compound([("scale", malformed)]))]),
            )]);
            assert_eq!(
                parse_render_offsets(Some(&component)),
                Some(DEFAULT_ITEM_RENDER_OFFSETS)
            );
        }
    }

    #[test]
    fn integer_tags_count_as_numbers() {
        let component = compound([(
            "main_hand",
            compound([(
                "third_person",
                compound([(
                    "rotation",
                    Nbt::List(vec![Nbt::Int(90), Nbt::Short(0), Nbt::Byte(-45)]),
                )]),
            )]),
        )]);
        let offsets = parse_render_offsets(Some(&component)).unwrap();
        assert_eq!(offsets.main_hand.third_person.rotation, [90.0, 0.0, -45.0]);
    }
}
