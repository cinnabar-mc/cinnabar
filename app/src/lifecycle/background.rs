//! Tells local worlds when the platform takes the app off screen and brings it back.
//!
//! A local server cannot pause, so the host saves and stops one that keeps running for a while
//! with the app hidden (see `launcher_host::local_worlds::Manager::app_hidden`). On return, a
//! session that was playing or joining that world ends with an explanation instead of a lost
//! connection. Desktop platforms never suspend the app.

use bevy::{prelude::*, window::AppLifecycle};
use client_shell::local_worlds::LocalWorlds;

use crate::{
    runtime::shutdown::{FatalError, record_fatal_error},
    session::SessionController,
};

/// Forwards lifecycle changes before the frame's network drain and menu systems, so the
/// explanation is the session's first recorded failure and the menu sees it first.
pub(crate) fn configure(app: &mut App) {
    app.add_message::<AppLifecycle>()
        .add_systems(PreUpdate, forward_app_visibility);
}

/// Whether `change` takes the app off screen (`Some(false)`) or brings it back (`Some(true)`).
fn visibility(change: AppLifecycle) -> Option<bool> {
    match change {
        AppLifecycle::WillSuspend | AppLifecycle::Suspended => Some(false),
        AppLifecycle::WillResume | AppLifecycle::Running => Some(true),
        AppLifecycle::Idle => None,
    }
}

/// Passes each visibility change to local worlds in order. When the host closed the world while
/// the app was hidden, a session playing or joining a local world ends with the explanation;
/// any other session is left alone. The closure is checked every frame, so a frame that runs
/// while the app is still hidden records the explanation before the session's own end can.
fn forward_app_visibility(
    mut lifecycle: MessageReader<AppLifecycle>,
    worlds: Option<ResMut<LocalWorlds>>,
    session: Option<Res<SessionController>>,
    fatal_error: Option<ResMut<FatalError>>,
) {
    let changes: Vec<bool> = lifecycle
        .read()
        .filter_map(|change| visibility(*change))
        .collect();
    let Some(mut worlds) = worlds else {
        return;
    };
    let mut closed = false;
    for visible in changes {
        if visible {
            closed |= worlds.app_shown();
        } else {
            worlds.app_hidden();
        }
    }
    if (closed || worlds.closed_while_hidden())
        && session.is_some_and(|session| session.is_local_world())
        && let Some(mut fatal_error) = fatal_error
    {
        record_fatal_error(
            &mut fatal_error.0,
            launcher::menu::disconnect::local_world_closed_in_background(),
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Duration;

    use bridge::WorldState;
    use launcher::local_worlds::world::{Generator, NewWorld};
    use launcher_host::local_worlds::Manager;

    use super::*;
    use crate::session::JoinStage;

    /// A manager with one world running on an in-memory server that closes after running hidden
    /// for `limit`, and the folder holding it.
    fn running_world(limit: Duration) -> (Manager, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("worlds folder");
        let manager =
            launcher_host::local_worlds::open_store_with_server(dir.path().join("worlds"), limit)
                .expect("manager");
        let world = manager
            .create(&NewWorld {
                name: "w".to_owned(),
                generator: Generator::Flat,
                backend: Some(bridge::Backend::Bds),
                ..NewWorld::default()
            })
            .expect("create");
        manager.open(&world.id, None).expect("open");
        test_time::eventually("the world to run", || {
            manager.status().state == WorldState::Running
        });
        (manager, dir)
    }

    /// An app holding `manager`'s worlds and a session that is `local` to them or remote.
    fn app(manager: &Manager, local: bool) -> App {
        let mut session = SessionController::default();
        // Kept by the join stage, so the join stays pending for the whole test.
        let (_launcher, ready) = crossbeam_channel::bounded(0);
        session.adopt_join("world", local, JoinStage::Launcher(ready));
        let mut app = App::new();
        app.insert_resource(LocalWorlds::with_manager(manager.clone()))
            .insert_resource(session)
            .init_resource::<FatalError>();
        configure(&mut app);
        app
    }

    fn fatal_error(app: &App) -> Option<String> {
        app.world().resource::<FatalError>().0.clone()
    }

    #[test]
    fn a_world_closed_while_hidden_ends_its_session_with_the_explanation() {
        let (manager, _dir) = running_world(Duration::ZERO);
        let mut app = app(&manager, true);
        app.world_mut().write_message(AppLifecycle::Running);
        app.update();
        assert_eq!(manager.status().state, WorldState::Running);

        app.world_mut().write_message(AppLifecycle::WillSuspend);
        app.update();
        test_time::eventually("the world to close", || {
            manager.status().state == WorldState::Idle
        });
        assert_eq!(
            fatal_error(&app),
            None,
            "nothing is reported before a frame runs"
        );
        // A frame that runs while the app is still hidden already explains the ended session.
        app.update();
        assert_eq!(
            fatal_error(&app),
            Some(launcher::menu::disconnect::local_world_closed_in_background())
        );
        app.world_mut().resource_mut::<FatalError>().0 = None;

        // Returning starts nothing: the player opens the world again.
        app.world_mut().write_message(AppLifecycle::WillResume);
        app.update();
        assert_eq!(
            fatal_error(&app),
            Some(launcher::menu::disconnect::local_world_closed_in_background())
        );
        assert_eq!(manager.status().state, WorldState::Idle);
    }

    #[test]
    fn a_short_absence_keeps_the_world_and_its_session() {
        let (manager, _dir) = running_world(Duration::from_secs(3600));
        let mut app = app(&manager, true);
        for change in [
            AppLifecycle::WillSuspend,
            AppLifecycle::Suspended,
            AppLifecycle::WillResume,
            AppLifecycle::Running,
        ] {
            app.world_mut().write_message(change);
            app.update();
        }
        assert_eq!(manager.status().state, WorldState::Running);
        assert_eq!(fatal_error(&app), None);
    }

    #[test]
    fn a_world_closed_while_hidden_leaves_a_remote_session_alone() {
        let (manager, _dir) = running_world(Duration::ZERO);
        let mut app = app(&manager, false);
        app.world_mut().write_message(AppLifecycle::Suspended);
        app.update();
        test_time::eventually("the world to close", || {
            manager.status().state == WorldState::Idle
        });
        app.world_mut().write_message(AppLifecycle::WillResume);
        app.update();
        assert_eq!(fatal_error(&app), None, "the remote session keeps running");
    }
}
