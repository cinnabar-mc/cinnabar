//! Serial replacements for Bevy's per-frame `ViewVisibility` bookkeeping.
//!
//! Before culling, Bevy moves each culled entity's current view visibility into its previous
//! bit; after culling, it hides the entities that were visible last frame but that no view
//! marked this frame. Both are parallel loops over every culled entity, chunk sections
//! included, that change one byte per entity, so nearly all of their frame cost was waking and
//! parking task pool threads. The replacements make the same transitions with the same change
//! detection, serially, in the sets Bevy's systems ran in.

use bevy::{
    app::{App, PostStartup, PostUpdate},
    camera::visibility::{NoCpuCulling, ViewVisibility, VisibilitySystems},
    ecs::schedule::{InternedSystemSet, ScheduleCleanupPolicy},
    prelude::{DetectChangesMut, IntoScheduleConfigs, Query, SystemSet, Without, World},
    reflect::tuple_struct::TupleStruct,
};

#[cfg(test)]
mod tests;

/// Swaps Bevy's parallel passes for the serial ones before the first `PostUpdate`.
pub(super) fn install(app: &mut App) {
    app.add_systems(PostStartup, replace_parallel_passes);
}

/// Removes Bevy's reset and hide passes and adds the serial ones to the same sets.
///
/// Bevy's reset shares its set with visibility propagation, so it is found as the one system
/// there that writes `ViewVisibility`. Panics when either pass is no longer where Bevy
/// scheduled it, so the replacement cannot go stale silently.
fn replace_parallel_passes(world: &mut World) {
    let view_visibility = world.register_component::<ViewVisibility>();
    world.schedule_scope(PostUpdate, |world, schedule| {
        schedule
            .initialize(world)
            .expect("PostUpdate builds before its visibility passes are replaced");
        let propagation = schedule
            .graph()
            .systems_in_set(VisibilitySystems::VisibilityPropagate.intern())
            .expect("Bevy schedules visibility propagation in PostUpdate");
        let resets: Vec<InternedSystemSet> = schedule
            .systems_with_access()
            .expect("PostUpdate was initialized above")
            .filter(|(key, system)| {
                propagation.contains(key)
                    && system
                        .access()
                        .to_filtered_access_set()
                        .combined_access()
                        .has_write(view_visibility)
            })
            .flat_map(|(_, system)| system.system().default_system_sets())
            .collect();
        let [reset] = resets[..] else {
            panic!("expected Bevy's one view-visibility reset in VisibilityPropagate: {resets:?}");
        };
        for set in [
            reset,
            VisibilitySystems::MarkNewlyHiddenEntitiesInvisible.intern(),
        ] {
            let removed = schedule.remove_systems_in_set(
                set,
                world,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            assert!(
                matches!(removed, Ok(1)),
                "Bevy's parallel visibility pass {set:?} is no longer scheduled alone: {removed:?}"
            );
        }
        schedule.add_systems((
            reset_view_visibility.in_set(VisibilitySystems::VisibilityPropagate),
            mark_newly_hidden_entities_invisible
                .in_set(VisibilitySystems::MarkNewlyHiddenEntitiesInvisible),
        ));
    });
}

/// An entity seen in the previous frame and not yet in this one: what Bevy's reset leaves
/// for a visible entity, and what its hide pass looks for. `ViewVisibility` keeps the current
/// frame in bit 0 and the previous one in bit 1; only reflection reaches that byte.
fn previously_visible() -> ViewVisibility {
    let mut state = ViewVisibility::HIDDEN;
    *state
        .field_mut(0)
        .and_then(|byte| byte.try_downcast_mut::<u8>())
        .expect("ViewVisibility reflects its state byte") = 0b10;
    state
}

/// Moves this frame's visibility into the previous-frame bit without change detection.
fn reset_view_visibility(mut entities: Query<&mut ViewVisibility, Without<NoCpuCulling>>) {
    let previously_visible = previously_visible();
    for mut visibility in &mut entities {
        let reset = if visibility.get() {
            previously_visible
        } else {
            ViewVisibility::HIDDEN
        };
        *visibility.bypass_change_detection() = reset;
    }
}

/// Hides, with change detection, the entities that were visible last frame and that no view
/// marked visible this frame.
fn mark_newly_hidden_entities_invisible(
    mut entities: Query<&mut ViewVisibility, Without<NoCpuCulling>>,
) {
    let previously_visible = previously_visible();
    for mut visibility in &mut entities {
        if *visibility == previously_visible {
            *visibility = ViewVisibility::HIDDEN;
        }
    }
}
