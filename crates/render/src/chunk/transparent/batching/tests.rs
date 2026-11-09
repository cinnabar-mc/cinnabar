use super::*;
use bevy::{
    prelude::{App, Entity},
    render::{
        render_phase::{AddRenderCommand, SetItemPipeline},
        render_resource::CachedRenderPipelineId,
        view::RetainedViewEntity,
    },
};
use std::ops::Range;

/// Registers real command identities without creating a graphics device.
fn fixture() -> (Entity, DrawFunctionId, DrawFunctionId) {
    let mut app = App::new();
    app.init_resource::<DrawFunctions<Transparent3d>>()
        .add_render_command::<Transparent3d, crate::chunk::DrawTransparentLiquidCommands>()
        .add_render_command::<Transparent3d, SetItemPipeline>();
    let functions = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read();
    let water = functions.id::<crate::chunk::DrawTransparentLiquidCommands>();
    let other = functions.id::<SetItemPipeline>();
    drop(functions);
    (app.world_mut().spawn_empty().id(), water, other)
}

/// Creates one queued water range with the production batching and sorting contracts.
fn item(entity: Entity, draw: DrawFunctionId, range: Range<u32>, distance: f32) -> Transparent3d {
    Transparent3d {
        sorting_info: TransparentSortingInfo3d::AlwaysOnTop,
        distance,
        pipeline: CachedRenderPipelineId::new(1),
        entity: (entity, entity.into()),
        draw_function: draw,
        batch_range: 0..1,
        extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
            range,
            batch_set_index: None,
        },
        indexed: true,
    }
}

/// Expands the submitted reference sequence so lost, duplicated or reordered geometry fails.
fn expanded(phase: &SortedRenderPhase<Transparent3d>) -> Vec<(DrawFunctionId, u32)> {
    phase
        .items
        .values()
        .flat_map(|item| {
            let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = &item.extra_index
            else {
                panic!("fixture needs a reference range");
            };
            range.clone().map(|index| (item.draw_function, index))
        })
        .collect()
}

#[test]
fn coalesced_water_keeps_particle_interleaving_gaps_and_stable_ties() {
    let (entity, water, particle) = fixture();
    let mut phase = SortedRenderPhase::default();
    for (draw, range, distance) in [
        (water, 0..2, -8.0),
        (water, 2..5, -7.0),
        (particle, 90..92, -6.0),
        (water, 5..7, -6.0),
        (water, 7..8, -5.0),
        (water, 10..12, -4.0),
        (water, 12..14, -3.0),
    ] {
        crate::transparent_phase::add(&mut phase, item(entity, draw, range, distance));
    }
    phase.sort();
    let expected = expanded(&phase);
    coalesce(&mut phase, water);
    assert_eq!(expanded(&phase), expected);
    assert_eq!(phase.items.len(), 4);
    assert_eq!(
        phase
            .items
            .values()
            .map(|item| item.distance)
            .collect::<Vec<_>>(),
        [-8.0, -6.0, -6.0, -4.0]
    );
    assert!(phase.items.values().all(|item| item.batch_range == (0..1)));
    coalesce(&mut phase, water);
    assert_eq!(expanded(&phase), expected);
    assert_eq!(phase.items.len(), 4);
}

#[test]
fn incompatible_draws_and_reversed_or_overlapping_ranges_stay_separate() {
    let (entity, water, other) = fixture();
    for case in 0..9 {
        let mut phase = SortedRenderPhase::default();
        crate::transparent_phase::add(&mut phase, item(entity, water, 0..2, -1.0));
        let mut next = item(entity, water, 2..4, 0.0);
        match case {
            0 => next.pipeline = CachedRenderPipelineId::new(2),
            1 => next.entity = (Entity::PLACEHOLDER, Entity::PLACEHOLDER.into()),
            2 => next.draw_function = other,
            3 => next.indexed = false,
            4 => next.batch_range = 0..2,
            5 => {
                next.sorting_info = TransparentSortingInfo3d::Sorted {
                    mesh_center: bevy::math::Vec3::ZERO,
                    depth_bias: 0.0,
                }
            }
            6 => {
                next.extra_index = PhaseItemExtraIndex::IndirectParametersIndex {
                    range: 1..3,
                    batch_set_index: None,
                }
            }
            7 => {
                next.extra_index = PhaseItemExtraIndex::IndirectParametersIndex {
                    range: 2..2,
                    batch_set_index: None,
                }
            }
            _ => next.extra_index = PhaseItemExtraIndex::None,
        }
        crate::transparent_phase::add(&mut phase, next);
        coalesce(&mut phase, water);
        assert_eq!(phase.items.len(), 2, "case {case}");
    }
    let mut phase = SortedRenderPhase::default();
    crate::transparent_phase::add(&mut phase, item(entity, water, 2..4, -1.0));
    crate::transparent_phase::add(&mut phase, item(entity, water, 0..2, 0.0));
    coalesce(&mut phase, water);
    assert_eq!(
        expanded(&phase),
        [(water, 2), (water, 3), (water, 0), (water, 1)]
    );
    assert_eq!(phase.items.len(), 2);
}

#[test]
fn compacted_transient_items_expire_before_the_next_frame() {
    let (entity, water, _) = fixture();
    let view = RetainedViewEntity::new(entity.into(), None, 0);
    let mut phases = ViewSortedRenderPhases::<Transparent3d>::default();
    phases.prepare_for_new_frame(view);
    let phase = phases.get_mut(&view).unwrap();
    crate::transparent_phase::add(phase, item(entity, water, 0..2, -1.0));
    crate::transparent_phase::add(phase, item(entity, water, 2..4, 0.0));
    coalesce(phase, water);
    assert_eq!(phase.items.len(), 1);
    phases.prepare_for_new_frame(view);
    assert!(phases[&view].items.is_empty());
}
