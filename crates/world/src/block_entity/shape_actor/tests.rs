use super::{BlockEntityNbt, NbtValue, ShapeActor, StateValue, state_identity};
use crate::NbtCompound;

/// Encodes an actor compound through the public wire ingestion boundary.
fn decode(root: NbtCompound) -> BlockEntityNbt {
    BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0
}

/// Builds a piston arm actor snapshot with the supplied state and progress tags.
fn piston(state: Option<NbtValue>) -> NbtCompound {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("PistonArm".into()));
    root.insert("Progress", NbtValue::Float(0.5));
    if let Some(state) = state {
        root.insert("State", state);
    }
    root
}

#[test]
fn piston_actors_take_only_a_typed_state_byte() {
    let state = |root| match decode(root).shape_actor() {
        ShapeActor::Piston { motion } => motion.state(),
        other => panic!("not a piston actor: {other:?}"),
    };
    assert_eq!(state(piston(Some(NbtValue::Byte(3)))), 3);
    assert_eq!(state(piston(Some(NbtValue::Int(3)))), 0);
    assert_eq!(state(piston(None)), 0);
    let progress = decode(piston(Some(NbtValue::Byte(2))))
        .piston_motion()
        .unwrap()
        .step_at(0)
        .progress;
    assert_eq!(progress, 0.0, "the reported progress is ignored");
}

#[test]
fn moving_state_identity_preserves_types_and_expansion_requires_a_byte() {
    let mut states = NbtCompound::default();
    states.insert("age_bit", NbtValue::Byte(1));
    let mut block = NbtCompound::default();
    block.insert("name", NbtValue::String("minecraft:oak_sapling".into()));
    block.insert("states", NbtValue::Compound(states));
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("MovingBlock".into()));
    root.insert("movingBlock", NbtValue::Compound(block));
    root.insert("pistonPosY", NbtValue::Int(-1));
    root.insert("expanding", NbtValue::Int(1));
    let identity = state_identity("minecraft:oak_sapling", [("age_bit", StateValue::Byte(1))]);
    assert_eq!(
        decode(root.clone()).shape_actor(),
        &ShapeActor::Moving {
            state_identity: identity,
            piston: [0, -1, 0],
            expanding: false
        }
    );
    root.insert("expanding", NbtValue::Byte(1));
    assert_eq!(
        decode(root).shape_actor(),
        &ShapeActor::Moving {
            state_identity: identity,
            piston: [0, -1, 0],
            expanding: true
        }
    );
    assert_ne!(
        identity,
        state_identity("minecraft:oak_sapling", [("age_bit", StateValue::Int(1))])
    );
}
