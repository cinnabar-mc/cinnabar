use super::*;
use render_model::block_entity::CopperAge;

/// A retained plant with typed NBT states, independent of inventory item metadata.
fn potted_nbt(name: &str, states: NbtCompound) -> NbtCompound {
    let mut plant = NbtCompound::default();
    plant.insert("name", world::NbtValue::String(name.into()));
    plant.insert("states", world::NbtValue::Compound(states));
    let mut root = NbtCompound::default();
    root.insert("PlantBlock", world::NbtValue::Compound(plant));
    root
}

#[test]
fn flower_pot_preserves_typed_plant_state_selectors() {
    let mut states = NbtCompound::default();
    states.insert("sapling_type", world::NbtValue::String("spruce".into()));
    states.insert("age_bit", world::NbtValue::Byte(0));
    states.insert("growth", world::NbtValue::Int(2));
    let Some(Template::FlowerPot { plant }) = describe(
        "FlowerPot",
        "minecraft:flower_pot",
        &BlockState::default(),
        &potted_nbt("minecraft:sapling", states),
        [0; 3],
    ) else {
        panic!("plant expected")
    };
    assert_eq!(plant.identifier.as_ref(), "minecraft:sapling");
    assert_eq!(
        plant.states["sapling_type"],
        serde_json::json!({"type":"string", "value":"spruce"})
    );
    assert_eq!(
        plant.states["age_bit"],
        serde_json::json!({"type":"byte", "value":0})
    );
    assert_eq!(
        plant.states["growth"],
        serde_json::json!({"type":"int", "value":2})
    );
}

#[test]
fn flower_pot_skips_empty_invalid_and_stale_contents() {
    for name in ["", "minecraft:air"] {
        assert!(
            describe(
                "FlowerPot",
                "minecraft:flower_pot",
                &BlockState::default(),
                &potted_nbt(name, NbtCompound::default()),
                [0; 3]
            )
            .is_none()
        );
    }
    let root = potted_nbt("minecraft:poppy", NbtCompound::default());
    assert!(
        describe(
            "FlowerPot",
            "minecraft:stone",
            &BlockState::default(),
            &root,
            [0; 3]
        )
        .is_none()
    );
    let mut states = NbtCompound::default();
    states.insert("growth", world::NbtValue::Float(2.0));
    assert!(
        describe(
            "FlowerPot",
            "minecraft:flower_pot",
            &BlockState::default(),
            &potted_nbt("minecraft:poppy", states),
            [0; 3]
        )
        .is_none()
    );
}

fn nbt(build: impl FnOnce(&mut Vec<u8>)) -> NbtCompound {
    let mut bytes = vec![10, 0];
    build(&mut bytes);
    bytes.push(0);
    world::BlockEntityNbt::decode_prefix(&bytes)
        .unwrap()
        .0
        .parse()
        .unwrap()
}

fn int_tag(out: &mut Vec<u8>, key: &str, zigzag: u8) {
    out.push(3);
    out.push(key.len() as u8);
    out.extend_from_slice(key.as_bytes());
    out.push(zigzag);
}

fn state(json: &str) -> BlockState {
    BlockState::parse(json)
}

/// Resolves a test's named backing from the real pinned palette.
fn describe(
    id: &str,
    name: &str,
    state: &BlockState,
    nbt: &NbtCompound,
    position: [i32; 3],
) -> Option<Template> {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let record = records.iter().find(|r| r.name.as_ref() == name)?;
    let backing = assets::facts::BlockEntityFacts::pinned().get(record.sequential_id)?;
    super::describe(id, backing, name, state, nbt, position)
}

#[test]
fn mismatched_block_entity_id_cannot_draw_on_a_different_backing() {
    let empty = NbtCompound::default();
    for (id, name) in [
        ("Banner", "minecraft:stone"),
        ("EnderChest", "minecraft:chest"),
        ("MobSpawner", "minecraft:trial_spawner"),
        ("Sign", "minecraft:oak_hanging_sign"),
    ] {
        assert!(describe(id, name, &BlockState::default(), &empty, [0; 3]).is_none());
    }
}

#[test]
fn a_broken_enchanting_table_leaves_no_floating_book() {
    let empty = nbt(|_| {});
    let none = state("{}");
    assert!(matches!(
        describe(
            "EnchantTable",
            "minecraft:enchanting_table",
            &none,
            &empty,
            [0; 3]
        ),
        Some(Template::EnchantTable)
    ));
    assert!(describe("EnchantTable", "minecraft:air", &none, &empty, [0; 3]).is_none());
    assert!(describe("EnchantTable", "minecraft:stone", &none, &empty, [0; 3]).is_none());
}

#[test]
fn current_player_head_identity_overrides_absent_or_stale_skull_type() {
    for compound in [nbt(|_| {}), nbt(|out| int_tag(out, "SkullType", 0))] {
        let template = describe(
            "Skull",
            "minecraft:player_head",
            &BlockState::default(),
            &compound,
            [0; 3],
        );
        assert!(matches!(
            template,
            Some(Template::Static(BlockEntityKind::Skull(SkullModel {
                kind: SkullKind::Player,
                ..
            })))
        ));
    }
    let legacy = nbt(|out| int_tag(out, "SkullType", 6));
    assert!(
        describe(
            "Skull",
            "minecraft:skull",
            &BlockState::default(),
            &legacy,
            [0; 3]
        )
        .is_none()
    );
    assert!(
        describe(
            "Skull",
            "custom:player_head",
            &BlockState::default(),
            &legacy,
            [0; 3]
        )
        .is_none()
    );
}

#[test]
fn review_campfire_items_keep_their_nbt_slots() {
    let compound = nbt(|out| {
        out.extend([10, 5]);
        out.extend(b"Item4");
        out.extend([8, 4]);
        out.extend(b"Name");
        let name = b"minecraft:apple";
        out.push(name.len() as u8);
        out.extend(name);
        out.push(0);
    });
    let Some(Template::Campfire { items, .. }) = describe(
        "Campfire",
        "minecraft:campfire",
        &BlockState::default(),
        &compound,
        [0; 3],
    ) else {
        panic!("expected campfire");
    };
    assert!(items[..3].iter().all(Option::is_none));
    assert_eq!(
        items[3].as_ref().unwrap().identifier.as_ref(),
        "minecraft:apple"
    );
}

#[test]
fn chest_pair_leads_by_flag_or_lower_coordinate() {
    // pairx = 5 (zigzag 10), pairz = 0.
    let plain = nbt(|out| {
        int_tag(out, "pairx", 10);
        int_tag(out, "pairz", 0);
    });
    assert_eq!(
        chest_pair([4, 64, 0], &plain),
        ChestPair::Lead {
            partner: [5, 64, 0]
        }
    );
    assert_eq!(chest_pair([6, 64, 0], &plain), ChestPair::Follower);
    assert_eq!(chest_pair([4, 64, 0], &nbt(|_| {})), ChestPair::Single);
}

#[test]
fn chest_variants_and_facing_resolve_from_the_block() {
    let template = describe(
        "Chest",
        "minecraft:waxed_exposed_copper_chest",
        &state(r#"{"minecraft:cardinal_direction":{"type":"string","value":"east"}}"#),
        &nbt(|_| {}),
        [0, 0, 0],
    );
    let Some(Template::Chest(model)) = template else {
        panic!("chest expected");
    };
    assert_eq!(model.variant, ChestVariant::Copper(CopperAge::Exposed));
    assert_eq!(model.facing, Facing::East);
    assert!(
        describe(
            "Chest",
            "minecraft:stone",
            &BlockState::default(),
            &nbt(|_| {}),
            [0; 3]
        )
        .is_none()
    );
}

#[test]
fn sign_text_needs_visible_characters_on_some_face() {
    let empty = nbt(|_| {});
    let standing = state(r#"{"ground_sign_direction":4}"#);
    assert!(describe("Sign", "minecraft:standing_sign", &standing, &empty, [0; 3]).is_none());
    let text = nbt(|out| {
        out.push(8);
        out.push(4);
        out.extend_from_slice(b"Text");
        out.push(2);
        out.extend_from_slice(b"hi");
    });
    let Some(Template::Sign { mount, front, back }) =
        describe("Sign", "minecraft:standing_sign", &standing, &text, [0; 3])
    else {
        panic!("legacy root text draws on the front");
    };
    assert_eq!(
        mount,
        SignMount::Standing {
            rotation_degrees: 90.0
        }
    );
    assert_eq!(
        front,
        Some(SignTextSpec::new(
            "hi".into(),
            DEFAULT_SIGN_COLOR,
            false,
            false
        ))
    );
    assert!(back.is_none());
}

#[test]
fn beds_read_color_from_nbt_and_half_and_direction_from_state() {
    // color byte 14 is red.
    let nbt = nbt(|out| {
        out.push(1);
        out.push(5);
        out.extend_from_slice(b"color");
        out.push(14);
    });
    let template = describe(
        "Bed",
        "minecraft:bed",
        &state(r#"{"direction":{"type":"int","value":3},"head_piece_bit":true}"#),
        &nbt,
        [0; 3],
    );
    assert_eq!(
        template,
        Some(Template::Static(BlockEntityKind::Bed(BedModel {
            color: "red",
            head: true,
            direction: 3,
        })))
    );
}

#[test]
fn the_default_dye_is_red_and_an_unknown_dye_draws_nothing() {
    let state = state(r#"{"direction":{"type":"int","value":0},"head_piece_bit":true}"#);
    assert_eq!(bed(&state, None).map(|model| model.color), Some("red"));
    assert_eq!(bed(&state, Some(99)), None);
}

#[test]
fn item_frames_preserve_all_six_outward_facing_states() {
    for (id, name, glow) in [
        ("ItemFrame", "minecraft:frame", false),
        ("GlowItemFrame", "minecraft:glow_frame", true),
    ] {
        let models = (0..6)
            .map(|outward| {
                let template = describe(
                    id,
                    name,
                    &state(&format!(
                        r#"{{"facing_direction":{{"type":"int","value":{outward}}}}}"#
                    )),
                    &nbt(|_| {}),
                    [0; 3],
                );
                let Some(Template::ItemFrame { model, item, .. }) = template else {
                    panic!("frame expected");
                };
                assert!(item.is_none());
                model
            })
            .collect::<Vec<_>>();
        for (outward, model) in models.iter().enumerate() {
            assert_eq!(usize::from(model.outward), outward);
            assert_eq!(model.glow, glow);
        }
    }
}

#[test]
fn item_frames_widen_for_the_map_and_photo_state_bits() {
    for (bits, full_face) in [
        ("", false),
        (r#","item_frame_map_bit":{"type":"byte","value":0}"#, false),
        (r#","item_frame_map_bit":{"type":"byte","value":1}"#, true),
        (r#","item_frame_photo_bit":{"type":"byte","value":1}"#, true),
    ] {
        let Some(Template::ItemFrame { model, .. }) = describe(
            "ItemFrame",
            "minecraft:frame",
            &state(&format!(
                r#"{{"facing_direction":{{"type":"int","value":3}}{bits}}}"#
            )),
            &nbt(|_| {}),
            [0; 3],
        ) else {
            panic!("frame expected");
        };
        assert_eq!(model.full_face, full_face, "state bits {bits:?}");
    }
}

#[test]
fn item_frame_rotation_accepts_float_degrees_and_legacy_byte_turns() {
    for step in 0..8_u8 {
        for floating in [false, true] {
            let rotation = nbt(|out| {
                out.push(if floating { 5 } else { 1 });
                out.push(12);
                out.extend_from_slice(b"ItemRotation");
                if floating {
                    out.extend_from_slice(
                        &(f32::from(step) * ITEM_FRAME_ROTATION_STEP_DEGREES).to_le_bytes(),
                    );
                } else {
                    out.push(step);
                }
                out.extend_from_slice(&[10, 4]);
                out.extend_from_slice(b"Item");
                out.extend_from_slice(&[8, 4]);
                out.extend_from_slice(b"Name");
                out.push(15);
                out.extend_from_slice(b"minecraft:clock");
                out.push(0);
            });
            let Some(Template::ItemFrame {
                rotation_degrees,
                item,
                ..
            }) = describe(
                "ItemFrame",
                "minecraft:frame",
                &state(r#"{"facing_direction":{"type":"int","value":2}}"#),
                &rotation,
                [0; 3],
            )
            else {
                panic!("frame expected");
            };
            assert_eq!(item.unwrap().identifier.as_ref(), "minecraft:clock");
            assert_eq!(
                rotation_degrees,
                f32::from(step) * ITEM_FRAME_ROTATION_STEP_DEGREES,
                "float degrees: {floating}"
            );
        }
    }
}

#[test]
fn item_frame_rotation_retains_fractional_degrees_and_defaults_unusable_values() {
    for (degrees, expected) in [
        (22.5_f32, 22.5),
        (-22.5, 337.5),
        (382.5, 22.5),
        (f32::NAN, 0.0),
        (f32::INFINITY, 0.0),
        (f32::NEG_INFINITY, 0.0),
    ] {
        let rotation = nbt(|out| {
            out.extend_from_slice(&[5, 12]);
            out.extend_from_slice(b"ItemRotation");
            out.extend_from_slice(&degrees.to_le_bytes());
        });
        assert_eq!(frame_rotation(&rotation), expected);
    }
    assert_eq!(frame_rotation(&nbt(|_| {})), 0.0);
}

#[test]
fn ids_without_a_renderer_draw_nothing() {
    assert!(
        describe(
            "Furnace",
            "minecraft:furnace",
            &BlockState::default(),
            &nbt(|_| {}),
            [0; 3]
        )
        .is_none()
    );
}

/// Resolves `probe:bus` to 7 and `probe:plate` to 9, anything else to air.
struct ProbeIds;

impl world::BlockIds for ProbeIds {
    fn air(&self) -> u32 {
        0
    }

    fn resolve(&self, network_id: u32) -> u32 {
        network_id
    }

    fn resolve_persistent(&self, entry: &NbtCompound) -> u32 {
        match entry.string("name") {
            Some("probe:bus") => 7,
            Some("probe:plate") => 9,
            _ => 0,
        }
    }
}

#[test]
fn overlay_items_show_only_on_their_host_and_leave_block_overlays_to_the_mesh() {
    use world::NbtValue;
    let named = |name: &str| {
        let mut state = NbtCompound::default();
        state.insert("name", NbtValue::String(name.into()));
        NbtValue::Compound(state)
    };
    let mut plate = NbtCompound::default();
    plate.insert("slot", NbtValue::Byte(0));
    plate.insert("face", NbtValue::Byte(2));
    plate.insert("block", named("probe:plate"));
    let mut stack = NbtCompound::default();
    stack.insert("Name", NbtValue::String("minecraft:diamond".into()));
    let mut diamond = NbtCompound::default();
    diamond.insert("slot", NbtValue::Byte(1));
    diamond.insert("face", NbtValue::Byte(1));
    diamond.insert("item", NbtValue::Compound(stack));
    for (key, value) in [("size", 6.4), ("x", 0.0), ("y", 0.8)] {
        diamond.insert(key, NbtValue::Float(value));
    }
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("CinnabarOverlays".into()));
    root.insert("host", named("probe:bus"));
    root.insert(
        "overlays",
        NbtValue::List(vec![NbtValue::Compound(plate), NbtValue::Compound(diamond)]),
    );
    let nbt = world::BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0;
    let overlays = nbt.cell_overlays().unwrap();
    let Some(Template::OverlayItems(items)) = super::overlay_items(overlays, &ProbeIds, 7) else {
        panic!("the host draws its diamond");
    };
    assert_eq!(
        *items,
        [(
            1,
            world::OverlayItem {
                name: Arc::from("minecraft:diamond"),
                damage: 0,
                size: 6.4,
                x: 0.0,
                y: 0.8,
            }
        )]
    );
    assert_eq!(super::overlay_items(overlays, &ProbeIds, 9), None);
}
