//! Counts camera cuts so history-based effects know when the previous frame is unrelated.

use bevy::prelude::*;
use semantic_input::PerspectiveMode;

use super::{CameraRig, CameraSettingsAuthority, ServerCameraView};
use crate::local_player::LocalViewPose;

/// Everything whose change moves the rendered view to an unrelated place.
#[derive(Clone, Copy, PartialEq)]
struct CameraAnchor {
    player: u64,
    server: u64,
    perspective: PerspectiveMode,
    rig: Option<CameraRig>,
    death_rotation: Option<Quat>,
}

/// The camera-cut epoch, advanced every frame the rendered view jumps; tracked whether or not
/// any effect consumes it.
#[derive(Resource, Default)]
pub struct CameraCuts {
    previous: Option<CameraAnchor>,
    epoch: u64,
}

impl CameraCuts {
    /// Changes exactly on frames whose view does not continue the previous frame's view.
    #[must_use]
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }
}

/// Advances the epoch on player or server reanchors, perspective, rig and death-view changes.
/// Runs after the camera pose resolves and before effects that read [`CameraCuts`].
pub fn track_camera_cuts(
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    server: Res<ServerCameraView>,
    mut cuts: ResMut<CameraCuts>,
) {
    let anchor = CameraAnchor {
        player: view.camera_reanchor_epoch(),
        server: server.camera_reanchor_epoch(),
        perspective: settings.perspective(),
        rig: settings.rig(),
        death_rotation: settings.death_rotation(),
    };
    if cuts.previous != Some(anchor) {
        cuts.previous = Some(anchor);
        cuts.epoch = cuts.epoch.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<CameraSettingsAuthority>()
            .init_resource::<LocalViewPose>()
            .init_resource::<ServerCameraView>()
            .init_resource::<CameraCuts>()
            .add_systems(Update, track_camera_cuts);
        app.update();
        app
    }

    fn epoch(app: &App) -> u64 {
        app.world().resource::<CameraCuts>().epoch()
    }

    /// Applies `cut` and checks that exactly one epoch advance follows it.
    fn assert_cut(app: &mut App, cut: impl FnOnce(&mut World)) {
        let before = epoch(app);
        cut(app.world_mut());
        app.update();
        assert_eq!(epoch(app), before.wrapping_add(1));
        app.update();
        assert_eq!(epoch(app), before.wrapping_add(1));
    }

    #[test]
    fn steady_view_keeps_the_epoch_without_marking_the_resource_changed() {
        let mut app = app();
        let first = epoch(&app);
        let changed = |app: &App| {
            app.world()
                .get_resource_ref::<CameraCuts>()
                .unwrap()
                .last_changed()
        };
        let tick = changed(&app);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(epoch(&app), first);
        assert_eq!(changed(&app), tick);
    }

    #[test]
    fn every_reanchor_source_advances_the_epoch_once() {
        let mut app = app();
        assert_cut(&mut app, |world| {
            world.resource_mut::<LocalViewPose>().reanchor_camera();
        });
        assert_cut(&mut app, |world| {
            world.resource_mut::<ServerCameraView>().clear();
        });
    }

    #[test]
    fn perspective_rig_and_death_view_changes_advance_the_epoch_once() {
        let mut app = app();
        let player = *app.world().resource::<LocalViewPose>();
        assert_cut(&mut app, |world| {
            world
                .resource_mut::<CameraSettingsAuthority>()
                .cycle_perspective();
        });
        assert_cut(&mut app, |world| {
            world
                .resource_mut::<CameraSettingsAuthority>()
                .set_rig(Some(CameraRig {
                    offset: Vec3::new(0.0, 1.0, 6.0),
                    roll_radians: 0.0,
                    fov_delta_degrees: 0.0,
                }));
        });
        assert_cut(&mut app, |world| {
            world
                .resource_mut::<CameraSettingsAuthority>()
                .set_rig(None);
        });
        for death_rotation in [Some(Quat::from_rotation_y(0.8)), None] {
            assert_cut(&mut app, |world| {
                world
                    .resource_mut::<CameraSettingsAuthority>()
                    .set_death_view(death_rotation);
            });
        }
        assert_eq!(*app.world().resource::<LocalViewPose>(), player);
    }
}
