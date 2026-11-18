//! Actor change colour reaches colour-mask layers through colour metadata, and a wolf's only once
//! tamed.
use std::sync::Arc;

use assets::{MolangQuery, MolangQueryHandler, RuntimeEntityAssets};
use protocol::{
    ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue, ActorSpawnEvent,
};

use crate::actor_store::ActorStore;
use actor_animation::{AttachableAnimationInput, AttachablesRuntime, test_support};

const WOLF_IDENTIFIER: &str = test_support::WOLF_IDENTIFIER;
const COLOR_KEY: u32 = protocol_ids::ActorDataKey::ColorIndex.wire_value();
const FLAGS_KEY: u32 = 0;
/// Bedrock's palette white, 0xF0F0F0, the colour of index 0 and of a wild wolf.
const WHITE: [f32; 4] = [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 0.0];
const TAMED: u64 = match MolangQuery::IsTamed.descriptor().handler {
    MolangQueryHandler::Flag(bit) => 1 << bit,
    _ => panic!("query.is_tamed reads an actor flag"),
};

/// Spawns a wolf whose colour-mask controller inherits the actor's color.
fn wolf(flags: u64, color: ActorMetadataValue) -> ActorStore {
    spawn(WOLF_IDENTIFIER, true, flags, color)
}

/// Spawns `identifier` with one controller that inherits the actor's color, drawn with a
/// colour-mask material when `color_mask` holds.
fn spawn(identifier: &str, color_mask: bool, flags: u64, color: ActorMetadataValue) -> ActorStore {
    let state = assets::EntityRenderMaterialState {
        color_mask,
        ..Default::default()
    };
    spawn_drawn_with(identifier, state, flags, color)
}

/// Spawns `identifier` with one three-slot controller that inherits the actor's color, drawn
/// with material `state`.
fn spawn_drawn_with(
    identifier: &str,
    state: assets::EntityRenderMaterialState,
    flags: u64,
    color: ActorMetadataValue,
) -> ActorStore {
    let mut compiled = test_support::counting_random_compiled(identifier);
    compiled.render.layers[0].color = None;
    compiled.render.layers[0].material_state = Some(state);
    // Multitexture samplers read the second and third slots; each slot keeps its own copy of
    // the first slot's candidates, as carriers lay them out.
    let slot = compiled.render.slots[0];
    let first = slot.first_candidate as usize;
    let candidates =
        compiled.render.candidates[first..first + usize::from(slot.candidate_count)].to_vec();
    compiled.render.candidates = candidates.repeat(3).into_boxed_slice();
    compiled.render.slots = (0..3u32)
        .map(|index| assets::EntityRenderSlot {
            first_candidate: index * u32::from(slot.candidate_count),
            ..slot
        })
        .collect();
    compiled.render.layers[0].slot_count = 3;
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets);
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: identifier.into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: vec![
                ActorMetadata {
                    key: FLAGS_KEY,
                    value: ActorMetadataValue::Flags(flags),
                },
                ActorMetadata {
                    key: COLOR_KEY,
                    value: color,
                },
            ]
            .into(),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    store
}

/// Applies a metadata transition and completes the next actor tick.
fn update(store: &mut ActorStore, flags: u64, color: ActorMetadataValue) {
    let tick = store.actor_rig(1).unwrap().completed_tick + 1;
    store.apply(
        1,
        tick,
        protocol::ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            tick,
            properties: Arc::from([]),
            metadata: vec![
                ActorMetadata {
                    key: FLAGS_KEY,
                    value: ActorMetadataValue::Flags(flags),
                },
                ActorMetadata {
                    key: COLOR_KEY,
                    value: color,
                },
            ]
            .into(),
        }),
    );
    store.advance_interpolation_ticks(1);
}

#[test]
fn wolf_collar_color_follows_byte_dye_updates_and_taming() {
    let mut store = wolf(TAMED, ActorMetadataValue::Byte(14));
    let red = [176.0 / 255.0, 46.0 / 255.0, 38.0 / 255.0, 0.0];
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, red);
    assert_eq!(store.render_frame(0.5).layers(1).unwrap()[0].color, red);
    update(
        &mut store,
        TAMED | (1 << 11) | (1 << 24),
        ActorMetadataValue::Byte(11),
    );
    let blue = [60.0 / 255.0, 68.0 / 255.0, 170.0 / 255.0, 0.0];
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, blue);
    assert_eq!(store.render_frame(0.75).layers(1).unwrap()[0].color, blue);
    update(&mut store, 0, ActorMetadataValue::Byte(14));
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, WHITE);
    update(&mut store, TAMED | (1 << 25), ActorMetadataValue::Byte(14));
    assert_eq!(store.render_frame(0.25).layers(1).unwrap()[0].color, red);
}

#[test]
fn wolf_collar_masks_byte_index_and_rejects_other_metadata_types() {
    let mut store = wolf(TAMED, ActorMetadataValue::Byte(-2));
    assert_eq!(
        store.actor_rig(1).unwrap().render[0].color,
        [176.0 / 255.0, 46.0 / 255.0, 38.0 / 255.0, 0.0]
    );
    for invalid in [ActorMetadataValue::Int(14), ActorMetadataValue::Float(14.0)] {
        update(&mut store, TAMED, invalid);
        assert_eq!(store.render_frame(0.5).layers(1).unwrap()[0].color, WHITE);
    }
}

#[test]
fn wolf_collar_dye_does_not_color_its_equipment() {
    let store = wolf(TAMED, ActorMetadataValue::Byte(14));
    let compiled = test_support::compiled_fixture();
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut runtime = AttachablesRuntime::new(assets);
    let snapshot = runtime
        .evaluate(
            "minecraft:test_item",
            store.get(1).unwrap(),
            &store.actor_rig(1).unwrap(),
            AttachableAnimationInput::default(),
        )
        .unwrap();
    assert_eq!(snapshot.render[0].color, [1.0; 4]);
}

#[test]
fn a_sheep_fleece_takes_its_colour_index_dye() {
    let store = spawn("minecraft:sheep", true, 0, ActorMetadataValue::Byte(14));
    let red = [176.0 / 255.0, 46.0 / 255.0, 38.0 / 255.0, 0.0];
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, red);
    assert_eq!(store.render_frame(0.5).layers(1).unwrap()[0].color, red);
    let white = spawn("minecraft:sheep", true, 0, ActorMetadataValue::Byte(0));
    assert_eq!(white.actor_rig(1).unwrap().render[0].color, WHITE);
}

#[test]
fn only_colour_mask_materials_take_the_actor_dye() {
    let store = spawn("minecraft:cat", false, 0, ActorMetadataValue::Byte(14));
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, [1.0; 4]);
}

#[test]
fn a_multitexture_dyeing_its_third_texture_takes_the_actor_dye() {
    let state = assets::EntityRenderMaterialState {
        multitexture: true,
        color_second_texture: true,
        ..Default::default()
    };
    let store = spawn_drawn_with("minecraft:cat", state, 0, ActorMetadataValue::Byte(14));
    let red = [176.0 / 255.0, 46.0 / 255.0, 38.0 / 255.0, 0.0];
    let layer = &store.actor_rig(1).unwrap().render[0];
    assert!(
        layer.multitexture.is_some(),
        "the fixture draws three samplers"
    );
    assert_eq!(layer.color, red);
    let undyed = assets::EntityRenderMaterialState {
        multitexture: true,
        ..Default::default()
    };
    let store = spawn_drawn_with("minecraft:cat", undyed, 0, ActorMetadataValue::Byte(14));
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, [1.0; 4]);
}
