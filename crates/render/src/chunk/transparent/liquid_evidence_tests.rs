use super::transparent::resident_transparent_allocation;
use super::*;
use crate::chunk::transparent::liquid::transparent_frame_draws_for_range;

#[test]
fn merged_water_range_records_each_drawn_allocation_once() {
    let tint = ChunkBiomeTintIdentity::new(2, 2);
    let identities = (0..3)
        .map(|x| {
            let start = x as u32 * 8;
            TransparentAllocationIdentity::new(
                SubChunkKey::new(0, 65 + x, 0, 65),
                7,
                start..start + 8,
                32 + start..36 + start,
                x as u32,
            )
        })
        .collect::<Vec<_>>();
    let key = ViewSortKey::try_new(
        [0.0; 3],
        identities.clone(),
        ChunkTextureAssetIdentity::new(1, 1),
        tint,
    )
    .unwrap();
    let state = committed_transparent_state(
        &key,
        vec![
            PackedTransparentDrawRef::new(0, 0),
            PackedTransparentDrawRef::new(2, 1),
            PackedTransparentDrawRef::new(1, 0),
            PackedTransparentDrawRef::new(4, 2),
        ],
    );
    let snapshot = state.committed().unwrap();
    let (device, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut arena = ChunkGpuArena::new(&RenderDevice::from(device));
    let mut allocations = Vec::new();
    for (index, identity) in identities.iter().enumerate() {
        let entity = Entity::from_bits(index as u64 + 1);
        let gpu = resident_transparent_allocation(identity, tint);
        allocations.push(FrameAllocationIdentity {
            entity,
            key: identity.key,
            generation: identity.mesh_generation,
        });
        if index == 1 {
            arena.retired_allocations.push(RetiredArenaAllocation {
                entity,
                identity: gpu,
                quad: None,
                geometry: None,
                biome: None,
                origin: None,
                release_epoch: None,
            });
        } else {
            let mut allocation = retirement_test_allocation();
            allocation.gpu = gpu;
            arena.allocations.insert(entity, allocation);
        }
    }
    let draws = transparent_frame_draws_for_range(snapshot, &arena, 0..3);
    assert_eq!(draws.len(), 2, "the merged range draws two distinct chunks");
    for allocation in &allocations[..2] {
        assert!(draws.contains(&(allocation.entity, *allocation)));
    }
    let probe = FrameProbe::begin(
        target_expectation(
            Instant::now(),
            allocations.iter().map(|a| (a.key, a.generation)),
        ),
        allocations.iter().map(|a| FrameInstanceIdentity {
            entity: a.entity,
            key: a.key,
            generation: a.generation,
        }),
        allocations.iter().map(|a| (*a, ChunkStreamMask::LIQUID)),
    );
    assert_eq!(
        probe.record_transparent_draw(snapshot.generation(), draws),
        2
    );
    let completed = probe.complete();
    assert_eq!(
        completed.transparent_sort_generation,
        snapshot.generation().get()
    );
    assert_eq!(
        completed.drawn_manifest.as_ref(),
        &[(identities[0].key, 7), (identities[1].key, 7)]
    );
    for range in [0..0, 4..5, Range { start: 3, end: 2 }] {
        assert!(transparent_frame_draws_for_range(snapshot, &arena, range).is_empty());
    }
}
