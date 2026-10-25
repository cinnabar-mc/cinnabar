//! The nearest-first draw order keeps every visible slot through storage growth, trims and eye
//! crossings.

use super::*;

/// The production draw order loses no sub-chunk past the 16- and 32-thousand slot capacities:
/// a first sort, an append without an eye crossing, a reallocated storage, a trim and a crossing
/// each draw exactly the frustum-visible enabled slots, in the order's ranks.
#[test]
fn nearest_first_order_draws_every_slot_through_growth_trims_and_crossings() {
    let Some(gpu) = Gpu::for_fixture("gpu terrain cull draw order") else {
        return;
    };
    let mut records = Vec::new();
    for x in -25..25 {
        for y in 0..16 {
            for z in -25..25 {
                let slot = records.len() as u32;
                records.push(if slot % 17 == 9 {
                    CullRecord::default()
                } else {
                    CullRecord::new(&CullRecordSource {
                        origin: [x * 16, y * 16, z * 16],
                        base_vertex: slot as i32 * 4,
                        bounds: [[0; 3], [16; 3]],
                        cube: slot * 8..slot * 8 + 8,
                        solid_ends: [1, 2, 3, 4, 5, 6],
                        ..Default::default()
                    })
                    .unwrap()
                });
            }
        }
    }
    assert_eq!(records.len(), 40_000);
    let enabled = |slots: usize| enabled_words(|slot| slot % 11 != 4, slots);
    let check = |culler: &Culler, order: &mut order::DrawOrder, slots: usize, eye: Vec3| {
        // The first slots hold the negative-x half of the grid.
        let camera = camera(eye, eye + Vec3::new(-150.0, -50.0, 60.0));
        order.update(&records[..slots], order::eye_sub_chunk(eye));
        if let Some((first, ranked)) = order.take_stale() {
            culler.storage.write_order(&gpu.queue, first, ranked);
        }
        let ranked = order.slots().to_vec();
        assert_eq!(ranked.len(), slots);
        assert_eq!(
            ranked.iter().copied().collect::<BTreeSet<_>>().len(),
            slots,
            "every slot holds exactly one rank"
        );
        let enabled = enabled(slots);
        let must = (0..slots)
            .filter(|&slot| {
                records[slot].is_live()
                    && model::slot_enabled(&enabled, slot)
                    && bevy_visible(&camera.frustum, records[slot].origin)
            })
            .collect::<BTreeSet<_>>();
        culler.set_history(&vec![0; slots]);
        let input = view_input(&camera, 0);
        let late = culler.run(&input, CullPhase::Late, None);
        let visible = culler.history();
        assert!(
            must.len() > 1_000,
            "the eye sees only {} sub-chunks",
            must.len()
        );
        assert!(
            must.is_subset(&visible),
            "{slots} slots lost a visible sub-chunk"
        );
        assert!(
            visible
                .difference(&must)
                .all(|&slot| on_frustum_boundary(&camera.frustum, records[slot].origin)),
            "{slots} slots drew a sub-chunk away from every frustum plane"
        );
        let camera_split = CullCamera::new(Some(eye.as_dvec3().to_array()));
        assert_eq!(
            late,
            reference_args(
                &records[..slots],
                &ranked,
                camera_split,
                INDEX_COUNTS,
                |slot| { visible.contains(&slot) }
            ),
            "{slots} slots compact in rank order"
        );
    };

    let eye = Vec3::new(8.5, 130.0, 8.5);
    let mut order = order::DrawOrder::default();
    let mut small = Culler::new(&gpu, &records[..30_000], &enabled(30_000));
    assert_eq!(small.storage.capacity, 32_768);
    small.slots = 20_000;
    check(&small, &mut order, 20_000, eye);
    small.slots = 30_000;
    check(&small, &mut order, 30_000, eye);

    // Growth past the capacity reallocates the storage, as `upload_records` does.
    let mut large = Culler::new(&gpu, &records, &enabled(records.len()));
    assert_eq!(large.storage.capacity, 65_536);
    order.mark_stale();
    check(&large, &mut order, 40_000, eye);
    large.slots = 35_000;
    check(&large, &mut order, 35_000, eye);
    large.slots = 40_000;
    check(
        &large,
        &mut order,
        40_000,
        eye + Vec3::new(40.0, 0.0, -24.0),
    );
}
