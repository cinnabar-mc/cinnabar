//! Coalesces contiguous water references only after the complete transparent order is known.

use bevy::{
    core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d},
    prelude::{Res, ResMut},
    render::render_phase::{
        DrawFunctionId, DrawFunctions, PhaseItemExtraIndex, SortedRenderPhase,
        ViewSortedRenderPhases,
    },
};

/// Merges water runs without moving them across another draw, pipeline or native layer.
pub(in crate::chunk) fn batch_adjacent_water(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    functions: Res<DrawFunctions<Transparent3d>>,
) {
    let Some(draw) = functions
        .read()
        .get_id::<crate::chunk::DrawTransparentLiquidCommands>()
    else {
        return;
    };
    for phase in phases.values_mut() {
        coalesce(phase, draw);
    }
}

/// Compacts consecutive compatible draws in place, retaining the first item's position.
fn coalesce(phase: &mut SortedRenderPhase<Transparent3d>, draw: DrawFunctionId) {
    let mut kept = 0_usize;
    for next in 0..phase.items.len() {
        let end = kept
            .checked_sub(1)
            .and_then(|previous| joined_end(&phase.items[previous], &phase.items[next], draw));
        if let Some(end) = end {
            if let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } =
                &mut phase.items[kept - 1].extra_index
            {
                range.end = end;
            }
        } else {
            phase.items.swap_indices(kept, next);
            kept += 1;
        }
    }
    // Transient keys may still name removed items; Bevy's next-frame cleanup tolerates that.
    phase.items.truncate(kept);
}

/// Returns the union's end only when drawing the union preserves the exact reference order.
fn joined_end(left: &Transparent3d, right: &Transparent3d, draw: DrawFunctionId) -> Option<u32> {
    if left.draw_function != draw
        || right.draw_function != draw
        || left.pipeline != right.pipeline
        || left.entity != right.entity
        || !left.indexed
        || !right.indexed
        || left.batch_range != (0..1)
        || right.batch_range != (0..1)
        || !matches!(left.sorting_info, TransparentSortingInfo3d::AlwaysOnTop)
        || !matches!(right.sorting_info, TransparentSortingInfo3d::AlwaysOnTop)
    {
        return None;
    }
    match (&left.extra_index, &right.extra_index) {
        (
            PhaseItemExtraIndex::IndirectParametersIndex {
                range: first,
                batch_set_index: None,
            },
            PhaseItemExtraIndex::IndirectParametersIndex {
                range: second,
                batch_set_index: None,
            },
        ) if !first.is_empty() && !second.is_empty() && first.end == second.start => {
            Some(second.end)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
