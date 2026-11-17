//! Publishes the loaded mods' fog suppression for the atmosphere publisher.

use bevy::prelude::*;

use crate::{
    environment::VisualFogSuppression,
    runtime::{shutdown::FatalError, world::ClientWorld},
};

pub(super) fn configure(app: &mut App) {
    app.init_resource::<VisualFogSuppression>();
    app.add_systems(
        Update,
        publish
            .after(super::drive_mod)
            .before(crate::environment::update_atmosphere_frame),
    );
}

/// Skips fog only while a world is shown and a mod holds the request; any mod may skip it.
fn publish(
    runtime: Option<Res<super::ModRuntime>>,
    world: Option<Res<ClientWorld>>,
    fatal_error: Option<Res<FatalError>>,
    mut fog: ResMut<VisualFogSuppression>,
) {
    let shown = world.as_deref().is_some_and(|world| world.stream.is_some())
        && fatal_error.as_deref().is_none_or(|fatal| fatal.0.is_none());
    let wanted = match runtime.as_deref() {
        Some(runtime) if shown && !runtime.suspended => (0..runtime.hosts.host_count())
            .map(|index| runtime.hosts.host(index).fog_suppression())
            .fold(mod_host::FogSuppression::default(), |all, own| {
                all.union(own)
            }),
        _ => mod_host::FogSuppression::default(),
    };
    if fog.0 != wanted {
        fog.0 = wanted;
    }
}
