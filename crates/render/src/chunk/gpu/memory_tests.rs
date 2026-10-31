use super::*;
use crate::chunk::gpu::idle::release_idle_arena;
use std::sync::atomic::Ordering;

/// Creates an isolated arena on a device with the requested optional features.
fn memory_arena(features: wgpu::Features) -> (RenderDevice, RenderQueue, ChunkGpuArena) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    });
    let device = RenderDevice::from(device);
    let arena = ChunkGpuArena::new(&device);
    (device, RenderQueue::new(queue), arena)
}

/// Grows a quad stream through the production migration path.
fn grow_quads(arena: &mut ChunkGpuArena, device: &RenderDevice, queue: &RenderQueue) {
    let copy_bytes = arena.quad_buffer.size();
    assert!(begin_arena_migration(
        arena,
        device,
        ArenaStream::Quads,
        ArenaGrowthPlan {
            new_capacity: 64,
            gpu_copy_bytes: copy_bytes,
        },
    ));
    advance_arena_migration(
        arena,
        TerrainUploads {
            device,
            queue,
            staging: None,
        },
        u64::MAX,
    );
    assert!(arena.migration.is_none());
}

/// Reads GPU bytes without depending on presentation or elapsed time.
fn read_buffer(device: &RenderDevice, queue: &RenderQueue, buffer: &Buffer) -> Vec<u8> {
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("terrain memory test readback"),
        size: buffer.size(),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, buffer.size());
    encoder.map_buffer_on_submit(&readback, wgpu::MapMode::Read, .., |result| {
        result.expect("terrain readback maps");
    });
    queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::Poll).unwrap();
    readback.slice(..).get_mapped_range().unwrap().to_vec()
}

/// Physical storage survives both retained references and unfinished submitted readers.
#[test]
fn idle_storage_waits_for_retirement_and_queue_completion_then_restarts_once() {
    let (device, queue, mut arena) = memory_arena(wgpu::Features::empty());
    grow_quads(&mut arena, &device, &queue);
    arena.indirect_capacity = 8;
    arena.indirect_buffer = create_indirect_buffer(&device, arena.indirect_capacity);
    arena.transparent_slot_refs = INITIAL_TRANSPARENT_SLOT_REFS * 2;
    arena.transparent_ref_buffer = transparent_ref_buffer(&device, arena.transparent_slot_refs);
    let large = arena.quad_buffer.id();
    let retirement =
        RetiredArenaAllocation::full(Entity::PLACEHOLDER, retirement_test_allocation());
    assert!(
        arena
            .retirement_budget
            .try_reserve(1, retirement.owned_bytes())
    );
    arena.retired_allocations.push(retirement);

    release_idle_arena(&mut arena, &device, &queue, false);
    assert_eq!(arena.quad_buffer.id(), large);
    assert!(arena.idle_completion.is_none());
    arena.retired_allocations[0].release_epoch = Some(2);
    release_completed_transparent_retirements(&mut arena, 1);
    release_idle_arena(&mut arena, &device, &queue, false);
    assert_eq!(arena.quad_buffer.id(), large);

    release_completed_transparent_retirements(&mut arena, 2);
    release_idle_arena(&mut arena, &device, &queue, false);
    let completion = arena.idle_completion.as_ref().unwrap().clone();
    // Model a callback that has not run, independent of the test backend's queue.
    completion.store(false, Ordering::Release);
    release_idle_arena(&mut arena, &device, &queue, false);
    assert_eq!(arena.quad_buffer.id(), large);
    completion.store(true, Ordering::Release);
    release_idle_arena(&mut arena, &device, &queue, false);
    assert_ne!(arena.quad_buffer.id(), large);
    assert_eq!(arena.quad_buffer.size(), PACKED_QUAD_BYTES);
    assert_eq!(
        arena.geometry_stream_buffer.size(),
        GEOMETRY_STREAM_WORD_BYTES
    );
    assert_eq!(arena.origin_buffer.size(), CHUNK_ORIGIN_BYTES);
    assert_eq!(
        arena.biome_buffer.size(),
        FALLBACK_BIOME_WORDS as u64 * BIOME_WORD_BYTES
    );
    assert!(arena.bind_group.is_none());
    assert!(arena.transparent_bind_group.is_none());
    assert!(arena.bind_group_buffers.is_none());
    assert_eq!(arena.indirect_buffer.size(), INDEXED_INDIRECT_BYTES);
    assert_eq!(arena.transparent_slot_refs, INITIAL_TRANSPARENT_SLOT_REFS);
    assert_eq!(arena.memory.idle_releases, 1);
    let minimal = arena.quad_buffer.id();
    release_idle_arena(&mut arena, &device, &queue, false);
    assert_eq!(
        arena.quad_buffer.id(),
        minimal,
        "idle frames do not reallocate"
    );
    assert_eq!(
        read_buffer(&device, &queue, &arena.biome_buffer),
        bytemuck::cast_slice::<_, u8>(&FALLBACK_BIOME_RECORD),
    );

    let bytes = [0x51; PACKED_QUAD_BYTES as usize];
    queue.write_buffer(&arena.quad_buffer, 0, &bytes);
    grow_quads(&mut arena, &device, &queue);
    assert_eq!(
        &read_buffer(&device, &queue, &arena.quad_buffer)[..bytes.len()],
        &bytes
    );
}

/// Reconnection and migration invalidate an earlier idle completion signal.
#[test]
fn new_work_cancels_idle_release_even_after_an_old_callback_completes() {
    let (device, queue, mut arena) = memory_arena(wgpu::Features::empty());
    grow_quads(&mut arena, &device, &queue);
    let large = arena.quad_buffer.id();
    release_idle_arena(&mut arena, &device, &queue, false);
    let old_completion = arena.idle_completion.as_ref().unwrap().clone();
    release_idle_arena(&mut arena, &device, &queue, true);
    assert!(arena.idle_completion.is_none());
    old_completion.store(true, Ordering::Release);
    release_idle_arena(&mut arena, &device, &queue, false);
    assert_eq!(arena.quad_buffer.id(), large);
    assert!(!Arc::ptr_eq(
        arena.idle_completion.as_ref().unwrap(),
        &old_completion
    ));

    assert!(begin_arena_migration(
        &mut arena,
        &device,
        ArenaStream::Quads,
        ArenaGrowthPlan {
            new_capacity: 128,
            gpu_copy_bytes: PACKED_QUAD_BYTES
        },
    ));
    release_idle_arena(&mut arena, &device, &queue, false);
    assert!(arena.idle_completion.is_none());
    assert_eq!(arena.quad_buffer.id(), large);
    assert!(arena.migration.is_some());
}

/// One failed local allocation retries with an unmapped host-visible buffer and keeps data.
#[test]
fn terrain_growth_falls_back_once_and_reports_the_allocation() {
    use std::{cell::Cell, rc::Rc};
    let (device, queue, mut arena) = memory_arena(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
    let source = [0x37; PACKED_QUAD_BYTES as usize];
    queue.write_buffer(&arena.quad_buffer, 0, &source);
    let attempts = Rc::new(Cell::new(0));
    let failures = crate::gpu_allocation::inject({
        let attempts = attempts.clone();
        move |_, _| {
            attempts.set(attempts.get() + 1);
            attempts.get() == 1
        }
    });
    grow_quads(&mut arena, &device, &queue);
    drop(failures);
    assert_eq!(attempts.get(), 2);
    assert!(arena.quad_buffer.usage().contains(BufferUsages::MAP_WRITE));
    assert_eq!(arena.memory.host_visible_attempts, 1);
    assert_eq!(arena.memory.host_visible_allocations, 1);
    assert_eq!(arena.memory.host_visible_bytes, arena.quad_buffer.size());
    assert_eq!(
        &read_buffer(&device, &queue, &arena.quad_buffer)[..source.len()],
        &source
    );
}

/// Unsupported fallback and failure of both placements retain the old arena for later retry.
#[test]
fn failed_growth_retries_only_supported_placements_and_preserves_the_old_buffer() {
    use std::{cell::Cell, rc::Rc};
    for (features, expected_attempts) in [
        (wgpu::Features::empty(), 1),
        (wgpu::Features::MAPPABLE_PRIMARY_BUFFERS, 2),
    ] {
        let (device, queue, mut arena) = memory_arena(features);
        let old = arena.quad_buffer.id();
        let attempts = Rc::new(Cell::new(0));
        let failures = crate::gpu_allocation::inject({
            let attempts = attempts.clone();
            move |_, _| {
                attempts.set(attempts.get() + 1);
                true
            }
        });
        let growth = ArenaGrowthPlan {
            new_capacity: 64,
            gpu_copy_bytes: PACKED_QUAD_BYTES,
        };
        assert!(!begin_arena_migration(
            &mut arena,
            &device,
            ArenaStream::Quads,
            growth
        ));
        drop(failures);
        assert_eq!(attempts.get(), expected_attempts);
        assert_eq!(arena.quad_buffer.id(), old);
        assert_eq!(arena.quad_capacity, 1);
        assert!(arena.migration.is_none());
        assert_eq!(arena.memory.host_visible_attempts, expected_attempts - 1);
        assert_eq!(arena.memory.host_visible_allocations, 0);
        grow_quads(&mut arena, &device, &queue);
        assert_eq!(arena.quad_capacity, growth.new_capacity);
        assert!(!arena.quad_buffer.usage().contains(BufferUsages::MAP_WRITE));
    }
}
