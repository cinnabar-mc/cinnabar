//! Actor lives: an identity that changes whenever an admitted actor spawns again or dies.

use super::WorldAuthority;
use crate::ActorSnapshot;

/// Most admitted actors at once: the actor store's capacity plus the client-fed local player,
/// which is admitted beyond it.
pub const MAX_ACTOR_LIVES: usize = crate::actor_store::MAX_TRACKED_ACTORS + 1;

/// An admitted actor's current life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorLife {
    /// Changes on every spawn of the runtime ID and every committed death; within a session a
    /// value never returns once it has changed.
    pub life: u64,
    /// The actor's current death state.
    pub dead: bool,
}

impl ActorSnapshot {
    /// The current life: the later of this lifetime's spawn and its latest death, both commit
    /// sequences of one session.
    #[must_use]
    pub fn life(&self) -> ActorLife {
        ActorLife {
            life: self.spawn_revision.max(self.status.death_revision),
            dead: self.status.dead,
        }
    }
}

impl WorldAuthority {
    /// Every admitted actor in the current dimension, including the local player, with its life;
    /// at most [`MAX_ACTOR_LIVES`].
    pub fn actor_lives(&self) -> impl Iterator<Item = (&ActorSnapshot, ActorLife)> {
        self.actors.actors().map(|actor| (actor, actor.life()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ActorEvent, ActorKind, ActorStatusKind, UiEvent, WorldBootstrap, WorldEvent};
    use std::sync::Arc;

    /// An isolated world with the local player 1 and a remote player 2; no game asset fixture.
    fn world() -> WorldAuthority {
        let mut world = WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            Arc::new(assets::RuntimeAssets::diagnostic()),
            None,
            [0.; 3],
            None,
        );
        world.sync_local_player_pose(&super::super::tests::local_health_feed());
        spawn(&mut world, 1, 2);
        world
    }

    /// Spawns runtime ID `runtime_id` again, with a fresh spawn revision.
    fn spawn(world: &mut WorldAuthority, sequence: u64, runtime_id: u64) {
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Spawn(protocol::ActorSpawnEvent {
                    dimension: 0,
                    unique_id: runtime_id as i64,
                    runtime_id,
                    kind: ActorKind::Player {
                        uuid: [runtime_id as u8; 16],
                        username: "Fixture".into(),
                    },
                    position: [1., 64., 1.],
                    velocity: [0.; 3],
                    pitch: 0.,
                    yaw: 0.,
                    head_yaw: 0.,
                    body_yaw: 0.,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
                Some(sequence),
            )
            .unwrap();
    }

    /// Commits one server status through the ordered boundary.
    fn status(world: &mut WorldAuthority, sequence: u64, runtime_id: u64, kind: ActorStatusKind) {
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Status(protocol::ActorStatusEvent {
                    runtime_id,
                    kind,
                    data: 0,
                })),
                Some(sequence),
            )
            .unwrap();
    }

    /// Commits server health through the attribute owner, without a death notice.
    fn health(world: &mut WorldAuthority, sequence: u64, runtime_id: u64, current: f32) {
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Attributes(
                    protocol::ActorAttributesUpdateEvent {
                        dimension: 0,
                        runtime_id,
                        tick: sequence,
                        attributes: Arc::from([protocol::ActorAttribute {
                            name: protocol_ids::Attribute::Health.wire_name().into(),
                            min: 0.,
                            max: 20.,
                            current,
                            default: None,
                            modifiers: Arc::from([]),
                        }]),
                    },
                )),
                Some(sequence),
            )
            .unwrap();
    }

    /// Commits the local player's HUD health.
    fn local_health(world: &mut WorldAuthority, sequence: u64, health: i32) {
        world
            .apply_ordered_event(
                WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health })),
                Some(sequence),
            )
            .unwrap();
    }

    /// Reads one admitted actor's retained life.
    fn life(world: &WorldAuthority, runtime_id: u64) -> Option<ActorLife> {
        world.actor(runtime_id).map(ActorSnapshot::life)
    }

    /// Respawns, removal and re-admission, death notices and health deaths each start a new life;
    /// other statuses, revival and repeated health keep it.
    #[test]
    fn remote_lives_change_exactly_where_counts_reset() {
        let mut world = world();
        let first = life(&world, 2).unwrap();
        assert!(!first.dead);
        status(&mut world, 2, 2, ActorStatusKind::Hurt);
        status(&mut world, 3, 2, ActorStatusKind::TotemActivate);
        health(&mut world, 4, 2, 5.);
        assert_eq!(life(&world, 2), Some(first));

        spawn(&mut world, 5, 2);
        let respawned = life(&world, 2).unwrap();
        assert_ne!(respawned.life, first.life, "a respawn starts a new life");

        status(&mut world, 6, 2, ActorStatusKind::Death);
        let died = life(&world, 2).unwrap();
        assert!(died.dead);
        assert_ne!(died.life, respawned.life);
        status(&mut world, 7, 2, ActorStatusKind::SpawnAlive);
        let revived = life(&world, 2).unwrap();
        assert!(!revived.dead);
        assert_eq!(
            revived.life, died.life,
            "revival continues the life death began"
        );

        health(&mut world, 8, 2, 0.);
        let attribute_death = life(&world, 2).unwrap();
        assert!(attribute_death.dead);
        assert_ne!(attribute_death.life, died.life);
        health(&mut world, 9, 2, 0.);
        let still_dead = life(&world, 2).unwrap();
        assert_ne!(
            still_dead.life, attribute_death.life,
            "each update that leaves the actor dead resets, as counts did"
        );
        health(&mut world, 10, 2, 20.);
        assert_eq!(life(&world, 2).unwrap().life, still_dead.life);
        assert!(!life(&world, 2).unwrap().dead);
        // No observation occurs between this death and revival.
        status(&mut world, 11, 2, ActorStatusKind::Death);
        status(&mut world, 12, 2, ActorStatusKind::SpawnAlive);
        let between_reads = life(&world, 2).unwrap();
        assert!(!between_reads.dead);
        assert_ne!(between_reads.life, still_dead.life);

        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Remove(protocol::ActorRemoveEvent {
                    dimension: 0,
                    unique_id: 2,
                })),
                Some(13),
            )
            .unwrap();
        assert_eq!(life(&world, 2), None);
        spawn(&mut world, 14, 2);
        let readmitted = life(&world, 2).unwrap();
        assert_ne!(readmitted.life, still_dead.life);
        assert!(!readmitted.dead);

        let mut seen = vec![
            first.life,
            respawned.life,
            died.life,
            attribute_death.life,
            still_dead.life,
            between_reads.life,
            readmitted.life,
        ];
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 7, "a changed life never returns");
    }

    /// The local player's HUD health, attributes and death notices start new lives as remote
    /// deaths do.
    #[test]
    fn local_lives_follow_hud_health_attributes_and_death_notices() {
        let mut world = world();
        let alive = life(&world, 1).unwrap();
        local_health(&mut world, 2, 20);
        assert_eq!(life(&world, 1), Some(alive));
        local_health(&mut world, 3, 0);
        let hud_death = life(&world, 1).unwrap();
        assert!(hud_death.dead);
        assert_ne!(hud_death.life, alive.life);
        local_health(&mut world, 4, 20);
        assert_eq!(
            life(&world, 1),
            Some(ActorLife {
                life: hud_death.life,
                dead: false
            })
        );
        health(&mut world, 5, 1, 0.);
        let attribute_death = life(&world, 1).unwrap();
        assert_ne!(attribute_death.life, hud_death.life);
        status(&mut world, 6, 1, ActorStatusKind::Death);
        assert_ne!(life(&world, 1).unwrap().life, attribute_death.life);
        let died = life(&world, 1).unwrap();
        world.sync_local_player_pose(&super::super::tests::local_health_feed());
        assert_eq!(life(&world, 1), Some(died), "pose feeds retain the life");
        local_health(&mut world, 7, -1);
        assert_ne!(life(&world, 1).unwrap().life, died.life);
    }

    /// Lives cover every admitted actor, including the local player; unknown IDs and actors left
    /// behind in another dimension have none.
    #[test]
    fn lives_cover_admitted_actors_of_the_current_dimension_only() {
        let mut world = world();
        let mut ids: Vec<_> = world
            .actor_lives()
            .map(|(actor, _)| actor.runtime_id)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, [1, 2]);
        assert_eq!(life(&world, 3), None);
        world.reset_dimension(2, 1);
        assert_eq!(life(&world, 2), None);
        spawn(&mut world, 3, 2);
        assert_eq!(
            life(&world, 2),
            None,
            "the previous dimension's actors stay unadmitted"
        );
        assert_eq!(world.actor_lives().count(), 0);
    }
    /// One collection after death and revival must retain the life from each commit.
    #[test]
    fn batched_combat_retains_lives_across_status_attribute_and_local_health_deaths() {
        for runtime_id in [1, 2] {
            for death_source in 0..3 {
                if runtime_id != 1 && death_source == 2 {
                    continue;
                }
                let mut world = world();
                let before = life(&world, runtime_id).unwrap().life;
                status(&mut world, 2, runtime_id, ActorStatusKind::TotemActivate);
                match death_source {
                    0 => status(&mut world, 3, runtime_id, ActorStatusKind::Death),
                    1 => health(&mut world, 3, runtime_id, 0.),
                    _ => local_health(&mut world, 3, 0),
                }
                let after = life(&world, runtime_id).unwrap().life;
                assert_ne!(before, after);
                status(&mut world, 4, runtime_id, ActorStatusKind::SpawnAlive);
                assert_eq!(life(&world, runtime_id).unwrap().life, after);
                status(&mut world, 5, runtime_id, ActorStatusKind::TotemActivate);
                let recorded: Vec<_> = world
                    .combat_evidence()
                    .map(|event| (event.kind, event.life))
                    .collect();
                let mut expected = vec![(super::super::ActorCombatKind::Totem, before)];
                if death_source == 0 {
                    expected.push((super::super::ActorCombatKind::Death, after));
                }
                expected.push((super::super::ActorCombatKind::Totem, after));
                assert_eq!(
                    recorded, expected,
                    "actor {runtime_id}, death source {death_source}"
                );
            }
        }
    }

    /// Respawn and runtime-ID reuse cannot relabel evidence still in the retained history.
    #[test]
    fn batched_combat_retains_lives_across_respawn_and_readmission() {
        let mut world = world();
        let first = life(&world, 2).unwrap().life;
        status(&mut world, 2, 2, ActorStatusKind::TotemActivate);
        spawn(&mut world, 3, 2);
        let respawned = life(&world, 2).unwrap().life;
        status(&mut world, 4, 2, ActorStatusKind::TotemActivate);
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Remove(protocol::ActorRemoveEvent {
                    dimension: 0,
                    unique_id: 2,
                })),
                Some(5),
            )
            .unwrap();
        spawn(&mut world, 6, 2);
        let readmitted = life(&world, 2).unwrap().life;
        status(&mut world, 7, 2, ActorStatusKind::TotemActivate);
        assert_ne!(first, respawned);
        assert_ne!(respawned, readmitted);
        assert_eq!(
            world
                .combat_evidence()
                .map(|event| event.life)
                .collect::<Vec<_>>(),
            [first, respawned, readmitted]
        );
    }
}
