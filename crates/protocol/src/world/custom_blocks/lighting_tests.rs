use super::{CustomVisualComponents, Nbt, visual_components};

fn geometry(flag: Option<Nbt>) -> Nbt {
    let mut fields = vec![("identifier".into(), Nbt::String("geometry.path".into()))];
    if let Some(flag) = flag {
        fields.push(("useBlockTypeLightAbsorption".into(), flag));
    }
    Nbt::Compound(vec![("minecraft:geometry".into(), Nbt::Compound(fields))])
}

#[test]
fn custom_geometry_absorption_defaults_to_zero_and_preserves_legacy_override() {
    for (flag, expected) in [
        (None, 0),
        (Some(Nbt::Byte(0)), 0),
        (Some(Nbt::Byte(1)), 15),
        (Some(Nbt::Byte(-1)), 15),
        (Some(Nbt::Int(1)), 0),
        (Some(Nbt::String("true".into())), 0),
    ] {
        let components = visual_components(Some(&geometry(flag)));
        assert_eq!(components.effective_light_dampening(), expected);
    }
    let scalar = Nbt::Compound(vec![(
        "minecraft:geometry".into(),
        Nbt::String("geometry.path".into()),
    )]);
    assert_eq!(
        visual_components(Some(&scalar)).effective_light_dampening(),
        0
    );
    assert_eq!(
        CustomVisualComponents::default().effective_light_dampening(),
        15
    );
}

#[test]
fn explicit_absorption_overrides_geometry_defaults() {
    for legacy in [false, true] {
        for dampening in [0, 6, 15, 255] {
            let components = CustomVisualComponents {
                geometry: Some("geometry.path".into()),
                geometry_use_block_type_light_absorption: legacy,
                light_dampening: Some(dampening),
                ..Default::default()
            };
            assert_eq!(components.effective_light_dampening(), dampening.min(15));
        }
    }
}

#[test]
fn legacy_top_rotation_requires_a_set_byte_on_the_geometry_component() {
    for (flag, expected) in [
        (None, false),
        (Some(Nbt::Byte(0)), false),
        (Some(Nbt::Byte(1)), true),
        (Some(Nbt::Int(1)), false),
    ] {
        let mut fields = vec![("identifier".into(), Nbt::String("geometry.path".into()))];
        if let Some(flag) = flag {
            fields.push(("needsLegacyTopRotation".into(), flag));
        }
        let components = Nbt::Compound(vec![("minecraft:geometry".into(), Nbt::Compound(fields))]);
        assert_eq!(
            visual_components(Some(&components)).geometry_needs_legacy_top_rotation,
            expected
        );
    }
}
