use super::coverage_tests::{device, meshes, populate};
use super::*;
use far_terrain::{
    HANDOFF_DEPTH_SHIFT, HANDOFF_DIRECTION_SHIFT, HANDOFF_DURATION, HANDOFF_FAR, HANDOFF_NEAR,
    HANDOFF_STEPS,
};

const START: Duration = Duration::from_secs(10);

/// Presentation time of an ordered sample within a transition that started at `START`.
fn at(step: u32) -> Duration {
    START + HANDOFF_DURATION * step / HANDOFF_STEPS
}

/// Reads the presentation byte of a column within an uploaded tile.
fn code(resident: &Resident, column: ColumnPos) -> u8 {
    let [x, z] = resident
        .mesh
        .key
        .origin()
        .map(|block| block / far_terrain::COLUMN_SIDE as i32);
    let side = resident.mesh.key.width_blocks() as usize / far_terrain::COLUMN_SIDE;
    resident.presentation[(column.z - z) as usize * side + (column.x - x) as usize]
}

/// Splits a presentation byte into its ordered opacity and depth rank.
fn split(code: u8) -> (u32, u32) {
    let code = u32::from(code);
    (
        code & ((1 << HANDOFF_DEPTH_SHIFT) - 1),
        code >> HANDOFF_DEPTH_SHIFT,
    )
}

/// The drawn surfaces over one ordered sample of a column, as `(tile, depth rank)`.
fn drawers(gpu: &FarTerrainGpu, column: ColumnPos, sample: u32) -> Vec<(TileKey, u32)> {
    gpu.resident
        .values()
        .filter(|resident| {
            resident.visible
                && resident.mesh.covers(column)
                && gpu.draws.iter().any(|draw| draw.slot == resident.slot)
        })
        .filter_map(|resident| {
            let (opacity, rank) = split(code(resident, column));
            (sample < opacity).then_some((resident.mesh.key, rank))
        })
        .collect()
}

/// Every ordered sample keeps a drawer, and overlapping drawers never share a depth rank.
fn assert_samples_covered(gpu: &FarTerrainGpu, old: &[Arc<TileMesh>]) {
    for column in old.iter().flat_map(|mesh| mesh.key.columns()) {
        for sample in 0..HANDOFF_STEPS {
            let mut ranks: Vec<_> = drawers(gpu, column, sample)
                .into_iter()
                .map(|(_, rank)| rank)
                .collect();
            assert!(!ranks.is_empty(), "sample {sample} lost {column:?}");
            ranks.sort_unstable();
            ranks.dedup();
            assert_eq!(
                ranks.len(),
                drawers(gpu, column, sample).len(),
                "coplanar drawers over {column:?}"
            );
        }
    }
}

/// A ready replacement fades in ahead of its solid predecessor, which retires only when whole.
#[test]
fn ready_lod_predecessors_stay_drawn_during_ordered_handover() {
    for reverse in [false, true] {
        let (device, queue) = device(256);
        let mut gpu = FarTerrainGpu::new(&device);
        let coarse = meshes([TileKey {
            level: 1,
            x: 0,
            z: 0,
        }]);
        let fine = meshes((0..2).flat_map(|x| (0..2).map(move |z| TileKey { level: 0, x, z })));
        let (old, replacement) = if reverse {
            (fine, coarse)
        } else {
            (coarse, fine)
        };
        assert!(gpu.apply_tiles_at(&device, &queue, &old, Some(START)));
        assert!(gpu.apply_tiles_at(&device, &queue, &replacement, Some(START)));
        for step in 0..HANDOFF_STEPS {
            gpu.advance_handoffs(&queue, at(step));
            assert_samples_covered(&gpu, &old);
            for column in old.iter().flat_map(|mesh| mesh.key.columns()) {
                let outgoing = old.iter().find(|mesh| mesh.covers(column)).unwrap();
                let incoming = replacement.iter().find(|mesh| mesh.covers(column)).unwrap();
                assert_eq!(
                    split(code(&gpu.resident[&outgoing.key], column)),
                    (HANDOFF_STEPS, 0)
                );
                assert_eq!(
                    split(code(&gpu.resident[&incoming.key], column)),
                    (step, 1),
                    "incoming draws its revealed samples just ahead of the predecessor"
                );
            }
        }
        assert!(gpu.advance_handoffs(&queue, at(HANDOFF_STEPS)));
        assert!(old.iter().all(|mesh| !gpu.resident.contains_key(&mesh.key)));
        assert_samples_covered(&gpu, &replacement);
        assert_eq!(gpu.draws.len(), replacement.len());
        let (_, uploads) = render_scene::runtime_profile_uploads::measure(|| {
            gpu.advance_handoffs(&queue, at(HANDOFF_STEPS) + HANDOFF_DURATION);
        });
        assert_eq!(uploads, (0, 0), "a settled transition stops writing");
    }
}

/// Reselecting the predecessor mid-fade commits nothing that leaves a sample uncovered.
#[test]
fn reversing_a_started_lod_fade_keeps_every_ordered_sample_covered() {
    let (device, queue) = device(256);
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes([TileKey {
        level: 1,
        x: 0,
        z: 0,
    }]);
    let replacement = meshes((0..2).flat_map(|x| (0..2).map(move |z| TileKey { level: 0, x, z })));
    gpu.apply_tiles_at(&device, &queue, &old, Some(START));
    gpu.apply_tiles_at(&device, &queue, &replacement, Some(START));
    gpu.advance_handoffs(&queue, at(HANDOFF_STEPS / 2));
    assert_samples_covered(&gpu, &old);
    gpu.apply_tiles_at(&device, &queue, &old, Some(at(HANDOFF_STEPS / 2)));
    assert_samples_covered(&gpu, &old);
    gpu.advance_handoffs(&queue, at(HANDOFF_STEPS * 3));
    assert_samples_covered(&gpu, &old);
    assert_eq!(gpu.resident.len(), old.len());
}

/// Geometry replaced by proven air fades out instead of vanishing in one frame.
#[test]
fn proven_empty_replacement_fades_its_predecessor_out() {
    let (device, queue) = device(4);
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes([TileKey {
        level: 2,
        x: 0,
        z: 0,
    }]);
    populate(&mut gpu, &device, &queue, &old);
    let column = old[0].key.columns().next().unwrap();
    gpu.set_empty_coverage(Some(&old[0].key.columns().collect()));
    assert!(gpu.apply_tiles_at(&device, &queue, &[], Some(START)));
    for step in 0..HANDOFF_STEPS {
        gpu.advance_handoffs(&queue, at(step));
        assert_eq!(
            split(code(&gpu.resident[&old[0].key], column)),
            (HANDOFF_STEPS - step, 0)
        );
        assert_eq!(gpu.draws.len(), 1);
    }
    gpu.advance_handoffs(&queue, at(HANDOFF_STEPS));
    assert!(gpu.draws.is_empty());
    assert!(gpu.covers_column(column));
}

/// Near/far changes reach every far tile over the column, ahead of near terrain's depth.
#[test]
fn near_and_far_changes_order_far_samples_ahead_of_near_terrain() {
    let (device, queue) = device(4);
    let mut gpu = FarTerrainGpu::new(&device);
    let tile = meshes([TileKey {
        level: 1,
        x: 0,
        z: 0,
    }]);
    populate(&mut gpu, &device, &queue, &tile);
    let mut columns = tile[0].key.columns();
    let (changing, steady) = (columns.next().unwrap(), columns.next().unwrap());
    for (direction, shown) in [
        (HANDOFF_NEAR, (|step| step) as fn(u32) -> u32),
        (HANDOFF_FAR, |step| HANDOFF_STEPS - step),
    ] {
        for step in 0..HANDOFF_STEPS {
            let packed = direction << HANDOFF_DIRECTION_SHIFT | step;
            gpu.publish_handoffs(&queue, std::iter::once((changing, packed)), at(step));
            let resident = &gpu.resident[&tile[0].key];
            assert_eq!(split(code(resident, changing)), (shown(step), 2));
            assert_eq!(split(code(resident, steady)), (HANDOFF_STEPS, 0));
        }
    }
    gpu.publish_handoffs(&queue, std::iter::empty(), at(HANDOFF_STEPS));
    assert_eq!(
        split(code(&gpu.resident[&tile[0].key], changing)),
        (HANDOFF_STEPS, 0)
    );
}

/// The render system starts tile fades on the extracted frame clock, not instantly.
#[test]
fn far_preparation_fades_on_the_frame_clock() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    let (device, queue) = device(256);
    let mut world = World::new();
    world.insert_resource(FarTerrainGpu::new(&device));
    world.insert_resource(device);
    world.insert_resource(queue);
    let mut time = Time::<()>::default();
    time.advance_to(START);
    world.insert_resource(time);
    let coarse = meshes([TileKey {
        level: 1,
        x: 0,
        z: 0,
    }]);
    let fine = meshes((0..2).flat_map(|x| (0..2).map(move |z| TileKey { level: 0, x, z })));
    world.insert_resource(FarTerrainScene {
        tiles: coarse.clone().into(),
        ..Default::default()
    });
    world.run_system_once(prepare_far_terrain).unwrap();
    world.resource_mut::<FarTerrainScene>().tiles = fine.clone().into();
    world.run_system_once(prepare_far_terrain).unwrap();
    assert!(
        world
            .resource::<FarTerrainGpu>()
            .resident
            .contains_key(&coarse[0].key)
    );
    assert_samples_covered(world.resource::<FarTerrainGpu>(), &coarse);
    world.resource_mut::<Time>().advance_to(at(HANDOFF_STEPS));
    world.run_system_once(prepare_far_terrain).unwrap();
    let gpu = world.resource::<FarTerrainGpu>();
    assert!(!gpu.resident.contains_key(&coarse[0].key));
    assert_eq!(gpu.draws.len(), fine.len());
}
