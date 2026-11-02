use super::*;
use client_shell::ui_resources::UiRuntimeResource;
use world::{NbtCompound, NbtValue};

#[test]
fn block_target_reads_selectable_cell_overlays() {
    let (mut world, _captured) = crate::block_use::published_tests::fixture();
    let state = |name: &str| {
        let mut state = NbtCompound::default();
        state.insert("name", NbtValue::String(name.into()));
        state.insert("states", NbtValue::Compound(NbtCompound::default()));
        NbtValue::Compound(state)
    };
    let mut overlay = NbtCompound::default();
    overlay.insert("slot", NbtValue::Byte(3));
    overlay.insert("face", NbtValue::Byte(2));
    overlay.insert("block", state("minecraft:stone"));
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("CinnabarOverlays".into()));
    root.insert("host", state("minecraft:air"));
    root.insert(
        "overlays",
        NbtValue::List(vec![NbtValue::Compound(overlay)]),
    );
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let sequence = stream.committed_sequence();
        stream
            .submit(
                sequence + 1,
                WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                    dimension: 0,
                    position: [4, 2, 6],
                    layer: 0,
                    network_id: stream.air_block_id(),
                }]),
            )
            .unwrap();
        stream
            .submit(
                sequence + 2,
                WorldEvent::BlockEntityUpdate(protocol::BlockEntityUpdateEvent {
                    dimension: 0,
                    position: [4, 2, 6],
                    nbt: root.encode_root().unwrap(),
                }),
            )
            .unwrap();
        test_time::eventually("target overlay commit", || {
            stream.poll([4.5, 2.5, 8.5], 0);
            stream.committed_sequence() >= sequence + 2
        });
    }
    let mut clock = client_presentation::environment::WorldClock::default();
    let mut weather = client_presentation::environment::WeatherState::default();
    client_presentation::environment::bind_session_generation(&mut clock, &mut weather, 7);
    world.insert_resource(clock);
    let point = world
        .run_system_cached(|context: GameplayContext, ui: Res<UiRuntimeResource>| {
            context.block_target(&ui)
        })
        .unwrap();
    assert_eq!(
        point,
        Some(GameplayVector3 {
            x: 4.5,
            y: 2.5,
            z: 6.5
        })
    );
}
