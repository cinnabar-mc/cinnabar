//! Native world-clock capture stays separate from the atmosphere's visual time override.

use super::*;
use crate::environment::VisualTimeOverride;
use client_presentation::environment::{self, WeatherState, WorldClock};

#[derive(Resource, Default)]
struct Observed(Option<mod_host::WorldTimeObservation>);

#[derive(Resource)]
struct Scope(mod_host::WorldObservation);

/// Captures through the production system parameter without requiring a camera or focused gameplay.
fn capture(context: GameplayContext, scope: Res<Scope>, mut observed: ResMut<Observed>) {
    observed.0 = context.world_time(&scope.0);
}

/// Builds an app whose session-7 server clock starts locked at day three, or at zero with a
/// running daylight cycle, as StartGame sets it.
fn app(daylight_cycle_enabled: bool) -> App {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    environment::replace_session(
        &mut clock,
        &mut weather,
        protocol::WorldEnvironmentBootstrap {
            initial_time: 5,
            day_cycle_lock_time: world::TICKS_PER_DAY as i32 * 3,
            daylight_cycle_enabled,
            weather_cycle_enabled: true,
            rain_level: 0.,
            lightning_level: 0.,
        },
        0.,
    );
    environment::bind_session_generation(&mut clock, &mut weather, 7);
    let mut app = App::new();
    app.insert_resource(clock)
        .insert_resource(Time::<()>::default())
        .insert_resource(VisualTimeOverride(Some(6000)))
        .insert_resource(Scope(mod_host::WorldObservation {
            session: 7,
            dimension: 0,
            world: "fixture".into(),
            position: (0., 64., 0.),
            alive: true,
            runtime_id: 1,
        }))
        .init_resource::<Observed>()
        .add_systems(Update, capture);
    app
}

/// A visual time override cannot change the server clock, and a stale session reads nothing.
#[test]
fn native_time_capture_ignores_time_changer_and_rejects_stale_sessions() {
    let mut app = app(false);
    let clock = *app.world().resource::<WorldClock>();
    app.update();
    let observed = app.world().resource::<Observed>().0.unwrap();
    assert_eq!(observed.session, 7);
    assert_eq!(observed.time, i64::from(world::TICKS_PER_DAY) * 3);
    assert!(!observed.daylight_cycle);
    assert_eq!(*app.world().resource::<WorldClock>(), clock);
    app.world_mut().resource_mut::<Scope>().0.session = 8;
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
}

/// A running daylight cycle advances absolute ticks at the native rate from the server's time.
#[test]
fn native_time_advances_while_the_daylight_cycle_runs() {
    let mut app = app(true);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(2));
    app.update();
    let observed = app.world().resource::<Observed>().0.unwrap();
    assert!(observed.daylight_cycle);
    assert_eq!(
        observed.time,
        2 * i64::from(mod_api::BEDROCK_TICKS_PER_SECOND)
    );
}
