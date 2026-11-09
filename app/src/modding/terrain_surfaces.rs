//! Publishes retained mod surface policies into the world's mesh generation.
use crate::runtime::{shutdown::FatalError, world::ClientWorld};
use bevy::prelude::*;
use client_state::ClientFrameSet;
use std::sync::Arc;

#[derive(Default)]
struct Cache {
    source: Vec<mod_api::terrain_surface::Rule>,
    scope: Option<(u64, u64, assets::NetworkIdMode)>,
    compiled: Arc<[meshing::surface::SurfaceRule]>,
}

/// Applies visual rules after callbacks and before presentation consumes world geometry.
pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        publish
            .after(ClientFrameSet::NetworkSend)
            .before(ClientFrameSet::UiPreparation),
    );
}

/// Resolves identifiers only when policy or the world's block-name registry changes.
fn publish(
    runtime: Option<Res<super::ModRuntime>>,
    mut world: Option<ResMut<ClientWorld>>,
    fatal: Option<Res<FatalError>>,
    mut cache: Local<Cache>,
) {
    let Some(stream) = world.as_mut().and_then(|world| world.stream.as_mut()) else {
        return;
    };
    let hosts = runtime.as_ref().filter(|runtime| {
        !runtime.suspended && fatal.as_ref().is_none_or(|fatal| fatal.0.is_none())
    });
    let source: Vec<_> = hosts
        .into_iter()
        .flat_map(|runtime| {
            (0..runtime.hosts.host_count())
                .flat_map(|index| runtime.hosts.host(index).terrain_surface_rules())
        })
        .collect();
    let scope = (
        stream.authority().actor_session_id(),
        stream.authority().block_name_revision(),
        stream.network_id_mode(),
    );
    if cache.scope != Some(scope) || !source.iter().copied().eq(cache.source.iter()) {
        cache.source = source.into_iter().cloned().collect();
        cache.scope = Some(scope);
        cache.compiled = cache
            .source
            .iter()
            .flat_map(|rule| rule.groups.iter().map(move |group| (rule, group)))
            .filter_map(|(rule, group)| {
                let mut states: Vec<_> = group
                    .iter()
                    .flat_map(|name| stream.authority().block_ids_for_name(name).iter().copied())
                    .collect();
                states.sort_unstable();
                states.dedup();
                (!states.is_empty()).then(|| meshing::surface::SurfaceRule {
                    states,
                    border: rule.border,
                    clauses: rule
                        .clauses
                        .iter()
                        .map(|clause| meshing::surface::CoverageClause {
                            tiles: clause.tiles,
                            conditions: clause
                                .conditions
                                .iter()
                                .map(|condition| meshing::surface::NeighbourCondition {
                                    offset: condition.offset,
                                    present: condition.present,
                                })
                                .collect(),
                        })
                        .collect(),
                })
            })
            .collect();
    }
    stream.set_surface_rules(cache.compiled.clone(), std::time::Instant::now());
}
