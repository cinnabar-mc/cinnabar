//! Publishes every admitted actor's life for `player-state.read-actor-lives`.

use mod_host::module_observations::{ActorLifeObservation, MAX_ACTOR_LIVES};
use std::sync::Arc;

const _: () = assert!(MAX_ACTOR_LIVES == client_world::MAX_ACTOR_LIVES);

/// Refills `published` with every admitted actor's life in `authority`, sorted by runtime ID,
/// and shares it with this frame's mods. Hosts release it after their callbacks, so the next
/// frame reuses its allocation.
pub(super) fn publish(
    published: &mut Arc<Vec<ActorLifeObservation>>,
    authority: Option<&client_world::WorldAuthority>,
) -> Arc<Vec<ActorLifeObservation>> {
    if Arc::get_mut(published).is_none() {
        *published = Arc::default();
    }
    let lives = Arc::get_mut(published).expect("a fresh publication is unshared");
    lives.clear();
    if let Some(authority) = authority {
        lives.extend(
            authority
                .actor_lives()
                .take(MAX_ACTOR_LIVES)
                .map(|(actor, life)| ActorLifeObservation {
                    runtime_id: actor.runtime_id,
                    player: matches!(actor.kind, protocol::ActorKind::Player { .. }),
                    life: mod_host::ActorLife {
                        life: life.life,
                        dead: life.dead,
                    },
                }),
        );
        lives.sort_unstable_by_key(|observed| observed.runtime_id);
    }
    Arc::clone(published)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Admits one remote actor of `kind` at `position` through the ordered world stream.
    fn spawn(
        stream: &mut chunk_pipeline::WorldStream,
        sequence: u64,
        runtime_id: u64,
        kind: protocol::ActorKind,
        position: [f32; 3],
    ) {
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(
                    protocol::ActorSpawnEvent {
                        dimension: 0,
                        unique_id: runtime_id as i64,
                        runtime_id,
                        kind,
                        position,
                        velocity: [0.; 3],
                        pitch: 0.,
                        yaw: 0.,
                        head_yaw: 0.,
                        body_yaw: 0.,
                        held_item: Default::default(),
                        metadata: [].into(),
                        attributes: [].into(),
                        properties: [].into(),
                        links: [].into(),
                    },
                )),
            )
            .unwrap();
    }

    /// Every admitted actor is published, however far away and however many, sorted with its
    /// kind and current life; the publication's allocation is reused once released.
    #[test]
    fn lives_cover_every_admitted_actor_beyond_the_actor_reads() {
        let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let count = 2 * mod_api::MAX_OBSERVED_ACTORS as u64;
        for index in 0..count {
            let runtime_id = count + 1 - index;
            let kind = if runtime_id.is_multiple_of(2) {
                protocol::ActorKind::Player {
                    uuid: [runtime_id as u8; 16],
                    username: "lives-fixture".into(),
                }
            } else {
                protocol::ActorKind::Entity {
                    identifier: "minecraft:zombie".into(),
                }
            };
            spawn(
                &mut stream,
                index + 1,
                runtime_id,
                kind,
                [1_000. * runtime_id as f32, 64., 0.],
            );
        }
        let mut published = Arc::default();
        let lives = publish(&mut published, Some(stream.authority()));
        assert_eq!(
            lives
                .iter()
                .map(|observed| observed.runtime_id)
                .collect::<Vec<_>>(),
            (2..=count + 1).collect::<Vec<_>>()
        );
        assert!(lives.iter().all(|observed| observed.player
            == (observed.runtime_id.is_multiple_of(2))
            && !observed.life.dead));
        let first = lives[0].life;
        drop(lives);
        let reused = Arc::as_ptr(&published);
        publish(&mut published, Some(stream.authority()));
        assert_eq!(Arc::as_ptr(&published), reused, "released lives are reused");

        stream
            .submit(
                count + 1,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Status(
                    protocol::ActorStatusEvent {
                        runtime_id: 2,
                        kind: protocol::ActorStatusKind::Death,
                        data: 0,
                    },
                )),
            )
            .unwrap();
        let lives = publish(&mut published, Some(stream.authority()));
        assert_eq!(lives[0].runtime_id, 2);
        assert!(lives[0].life.dead);
        assert_ne!(lives[0].life.life, first.life, "a death starts a new life");
        drop(lives);
        assert!(publish(&mut published, None).is_empty());
    }
}
