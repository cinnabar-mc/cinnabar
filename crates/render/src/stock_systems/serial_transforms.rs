//! Serial replacement for Bevy's transform sync of entities outside any hierarchy.
//!
//! Bevy copies `Transform` into `GlobalTransform` for every changed parentless, childless entity
//! in a parallel loop, so a frame in which only the camera moved still woke and parked task pool
//! threads. The replacement makes the same writes, with the same change detection, serially and
//! at the same point of the propagation chain.

use bevy::{
    app::{App, PostStartup, PostUpdate},
    ecs::schedule::ScheduleCleanupPolicy,
    prelude::{
        Added, Changed, ChildOf, Children, DetectChanges, GlobalTransform, IntoScheduleConfigs,
        IntoSystemSet, Or, ParamSet, Query, Ref, RemovedComponents, Transform, Without, World,
    },
    transform::{
        plugins::TransformSystems,
        systems::{propagate_parent_transforms, sync_simple_transforms},
    },
};

#[cfg(test)]
mod tests;

/// Swaps Bevy's parallel sync for the serial one before the first `PostUpdate`.
pub(super) fn install(app: &mut App) {
    app.add_systems(PostStartup, replace_parallel_sync);
}

/// Removes Bevy's sync and adds the serial one after parent propagation, where Bevy chained it.
/// Panics when Bevy no longer schedules the sync there, so the replacement cannot go stale.
fn replace_parallel_sync(world: &mut World) {
    world.schedule_scope(PostUpdate, |world, schedule| {
        let removed = schedule.remove_systems_in_set(
            sync_simple_transforms.into_system_set(),
            world,
            ScheduleCleanupPolicy::RemoveSystemsOnly,
        );
        assert!(
            matches!(removed, Ok(1)),
            "Bevy's parallel transform sync is no longer scheduled once in PostUpdate: {removed:?}"
        );
        schedule.add_systems(
            sync_simple_transforms_serially
                .after(propagate_parent_transforms)
                .in_set(TransformSystems::Propagate),
        );
    });
}

/// Changed or newly added entities outside any hierarchy.
type ChangedSimple<'w, 's> = Query<
    'w,
    's,
    (&'static Transform, &'static mut GlobalTransform),
    (
        Or<(Changed<Transform>, Added<GlobalTransform>)>,
        Without<ChildOf>,
        Without<Children>,
    ),
>;

/// Every entity outside any hierarchy, for those that just lost their parent.
type AnySimple<'w, 's> = Query<
    'w,
    's,
    (Ref<'static, Transform>, &'static mut GlobalTransform),
    (Without<ChildOf>, Without<Children>),
>;

/// Bevy's `sync_simple_transforms` without the parallel iteration: changed or newly added
/// entities outside any hierarchy, then entities that just lost their parent.
fn sync_simple_transforms_serially(
    mut query: ParamSet<(ChangedSimple, AnySimple)>,
    mut orphaned: RemovedComponents<ChildOf>,
) {
    for (transform, mut global_transform) in &mut query.p0() {
        *global_transform = GlobalTransform::from(*transform);
    }
    let mut query = query.p1();
    let mut iter = query.iter_many_mut(orphaned.read()).matched();
    while let Some((transform, mut global_transform)) = iter.fetch_next() {
        if !transform.is_changed() && !global_transform.is_added() {
            *global_transform = GlobalTransform::from(*transform);
        }
    }
}
