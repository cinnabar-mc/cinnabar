//! Read-only retained server damage evidence; attacker identity is never inferred here.
use super::WorldAuthority;

/// Resource ceiling for replayable accepted actor status notices.
const MAX_COMBAT_EVIDENCE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorCombatKind {
    Hurt,
    Death,
    /// A totem of undying activated on the actor, local or remote.
    Totem,
}

/// A server actor status accepted by this session's authoritative store.
#[derive(Clone, Copy, Debug)]
pub struct ActorCombatEvidence {
    pub revision: u64,
    pub ingress_sequence: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub player: bool,
    pub position: [f32; 3],
    pub kind: ActorCombatKind,
}

impl WorldAuthority {
    /// Borrows bounded accepted status history without consuming particle/audio notice queues.
    pub fn combat_evidence(&self) -> impl Iterator<Item = &ActorCombatEvidence> {
        self.combat_evidence.iter()
    }

    /// Records only accepted status updates with finite retained actor pose.
    pub(super) fn record_combat_evidence(
        &mut self,
        ingress_sequence: u64,
        runtime_id: u64,
        kind: ActorCombatKind,
    ) {
        let Some(actor) = self.actors.get(runtime_id) else {
            return;
        };
        if !actor.position.iter().all(|v| v.is_finite()) {
            return;
        }
        let Some(revision) = self.combat_revision.checked_add(1) else {
            return;
        };
        self.combat_revision = revision;
        if self.combat_evidence.len() == MAX_COMBAT_EVIDENCE {
            self.combat_evidence.pop_front();
        }
        self.combat_evidence.push_back(ActorCombatEvidence {
            revision,
            ingress_sequence,
            dimension: self.current_dimension,
            runtime_id,
            player: matches!(actor.kind, protocol::ActorKind::Player { .. }),
            position: actor.position,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ActorEvent, ActorKind, ActorStatusKind, WorldBootstrap, WorldEvent};
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
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Spawn(protocol::ActorSpawnEvent {
                    dimension: 0,
                    unique_id: 2,
                    runtime_id: 2,
                    kind: ActorKind::Player {
                        uuid: [2; 16],
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
                Some(1),
            )
            .unwrap();
        world
    }

    /// Commits one status notice through the ordered event boundary.
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

    /// Totem activations on the local and remote players join hurt and death evidence in order,
    /// without any counting interval to enable.
    #[test]
    fn totem_activations_are_combat_evidence_for_local_and_remote_actors() {
        let mut world = world();
        status(&mut world, 2, 2, ActorStatusKind::TotemActivate);
        status(&mut world, 3, 1, ActorStatusKind::TotemActivate);
        status(&mut world, 4, 2, ActorStatusKind::Hurt);
        status(&mut world, 5, 2, ActorStatusKind::SpawnAlive);
        status(&mut world, 6, 9, ActorStatusKind::TotemActivate);
        let events: Vec<_> = world
            .combat_evidence()
            .map(|event| (event.runtime_id, event.kind, event.player))
            .collect();
        assert_eq!(
            events,
            [
                (2, ActorCombatKind::Totem, true),
                (1, ActorCombatKind::Totem, true),
                (2, ActorCombatKind::Hurt, true),
            ],
            "unrelated statuses and unknown actors record nothing"
        );
        let revisions: Vec<_> = world
            .combat_evidence()
            .map(|event| event.revision)
            .collect();
        assert!(revisions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            world.combat_evidence().next().unwrap().position,
            [1., 64., 1.]
        );
    }
}
