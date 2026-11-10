//! The serial passes leave every entity's view visibility, and its change detection, exactly as
//! Bevy's parallel passes did.

use super::*;
use bevy::{
    asset::AssetPlugin,
    camera::{CameraPlugin, visibility::SetViewVisibility},
    image::ImagePlugin,
    mesh::MeshPlugin,
    platform::collections::HashMap,
    prelude::*,
    window::WindowPlugin,
};

/// Marks its entity visible in `frame` order, as some view's culling would.
#[derive(Component)]
struct SeenIn(&'static [bool]);

/// Per entity, per frame: the state byte and whether the frame changed it.
#[derive(Resource, Default)]
struct Observed(HashMap<Entity, Vec<(u8, bool)>>);

#[derive(Resource, Default)]
struct Frame(usize);

fn mark_seen(frame: Res<Frame>, mut seen: Query<(&SeenIn, &mut ViewVisibility)>) {
    for (seen_in, mut visibility) in &mut seen {
        if seen_in.0.get(frame.0).copied().unwrap_or(false) {
            visibility.set_visible();
        }
    }
}

fn observe(
    mut frame: ResMut<Frame>,
    mut observed: ResMut<Observed>,
    entities: Query<(Entity, Ref<ViewVisibility>)>,
) {
    for (entity, visibility) in &entities {
        let mut state = *visibility;
        let bits = *state
            .field_mut(0)
            .and_then(|byte| byte.try_downcast_mut::<u8>())
            .unwrap();
        observed
            .0
            .entry(entity)
            .or_default()
            .push((bits, visibility.is_changed()));
    }
    frame.0 += 1;
}

fn state(bits: u8) -> ViewVisibility {
    let mut state = ViewVisibility::HIDDEN;
    *state
        .field_mut(0)
        .and_then(|byte| byte.try_downcast_mut::<u8>())
        .unwrap() = bits;
    state
}

/// Runs four frames over every starting state, visibility and per-frame view verdict.
fn observe_frames(serial: bool) -> Vec<Vec<(u8, bool)>> {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins((
            AssetPlugin::default(),
            ImagePlugin::default(),
            MeshPlugin,
            TransformPlugin,
            CameraPlugin,
        ))
        .init_resource::<Observed>()
        .init_resource::<Frame>()
        .add_systems(
            PostUpdate,
            mark_seen.in_set(VisibilitySystems::CheckVisibility),
        )
        .add_systems(Last, observe);
    if serial {
        install(&mut app);
    }
    const VERDICTS: [&[bool]; 5] = [
        &[false, false, false, false],
        &[true, true, true, true],
        &[true, false, true, false],
        &[false, true, false, true],
        &[true, true, false, false],
    ];
    let mut entities = Vec::new();
    for bits in 0..4 {
        for verdicts in VERDICTS {
            for visibility in [Visibility::Visible, Visibility::Hidden] {
                for opted_out in [false, true] {
                    let mut entity = app.world_mut().spawn((
                        visibility,
                        state(bits),
                        SeenIn(verdicts),
                        Transform::default(),
                    ));
                    if opted_out {
                        entity.insert(NoCpuCulling);
                    }
                    entities.push(entity.id());
                }
            }
        }
    }
    for _ in 0..4 {
        app.update();
    }
    let observed = &app.world().resource::<Observed>().0;
    entities
        .iter()
        .map(|entity| observed[entity].clone())
        .collect()
}

#[test]
fn serial_passes_match_bevys_parallel_visibility_bookkeeping() {
    let parallel = observe_frames(false);
    assert!(
        parallel.iter().any(|frames| frames
            .iter()
            .any(|&(bits, changed)| bits == 0b11 && !changed)),
        "the fixture keeps entities visible across frames without change detection"
    );
    assert!(
        parallel
            .iter()
            .any(|frames| frames.iter().any(|&(bits, changed)| bits == 0 && changed)),
        "the fixture hides entities no view saw"
    );
    assert_eq!(observe_frames(true), parallel);
}
