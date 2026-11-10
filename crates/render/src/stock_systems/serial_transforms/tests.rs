//! The serial sync leaves every entity's global transform, and its change detection, exactly as
//! Bevy's parallel sync did.

use super::*;
use bevy::{
    platform::collections::HashMap,
    prelude::{Entity, Last, MinimalPlugins, ResMut, Resource, TransformPlugin},
};

/// Per entity, per frame: the global translation and whether the frame changed it.
#[derive(Resource, Default)]
struct Observed(HashMap<Entity, Vec<([f32; 3], bool)>>);

fn observe(mut observed: ResMut<Observed>, entities: Query<(Entity, Ref<GlobalTransform>)>) {
    for (entity, global) in &entities {
        observed
            .0
            .entry(entity)
            .or_default()
            .push((global.translation().to_array(), global.is_changed()));
    }
}

/// Runs five frames of moved, still, parented, orphaned and newly spawned entities.
fn observe_frames(serial: bool) -> Vec<Vec<([f32; 3], bool)>> {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .init_resource::<Observed>()
        .add_systems(Last, observe);
    if serial {
        install(&mut app);
    }
    let world = app.world_mut();
    let still = world.spawn(Transform::from_xyz(1.0, 0.0, 0.0)).id();
    let moving = world.spawn(Transform::from_xyz(2.0, 0.0, 0.0)).id();
    let parent = world.spawn(Transform::from_xyz(3.0, 0.0, 0.0)).id();
    let child = world
        .spawn((Transform::from_xyz(0.0, 1.0, 0.0), ChildOf(parent)))
        .id();
    let orphan_parent = world.spawn(Transform::from_xyz(5.0, 0.0, 0.0)).id();
    let orphan = world
        .spawn((Transform::from_xyz(0.0, 2.0, 0.0), ChildOf(orphan_parent)))
        .id();
    let mut entities = vec![still, moving, parent, child, orphan_parent, orphan];
    for frame in 0..5 {
        let world = app.world_mut();
        match frame {
            1 => {
                world.get_mut::<Transform>(moving).unwrap().translation.x += 1.0;
                world.entity_mut(orphan).remove::<ChildOf>();
            }
            2 => entities.push(world.spawn(Transform::from_xyz(7.0, 0.0, 0.0)).id()),
            3 => {
                world.get_mut::<Transform>(moving).unwrap().translation.y += 1.0;
                world.get_mut::<Transform>(parent).unwrap().translation.z += 1.0;
            }
            _ => {}
        }
        app.update();
    }
    let observed = &app.world().resource::<Observed>().0;
    entities
        .iter()
        .map(|entity| observed[entity].clone())
        .collect()
}

#[test]
fn serial_sync_matches_bevys_parallel_sync() {
    let parallel = observe_frames(false);
    assert_eq!(
        parallel[5].last().unwrap().0,
        [0.0, 2.0, 0.0],
        "the fixture reaches the orphaned-entity path"
    );
    assert_eq!(observe_frames(true), parallel);
}
