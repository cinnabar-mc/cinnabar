//! Maps mod player-list spans, keyed by actor runtime ID, to roster entries.

use std::{collections::HashMap, sync::Arc};
use ui::mod_presentation::{Presentation, Spans};

/// Maps each mod's player-list spans from player runtime IDs to the roster UUIDs of players
/// whose actors this session knows; other runtime IDs draw nothing.
pub(super) fn annotations(
    presentation: &Presentation,
    stream: Option<&chunk_pipeline::WorldStream>,
) -> HashMap<[u8; 16], Arc<Spans>> {
    let Some(authority) = stream.map(|stream| stream.authority()) else {
        return HashMap::new();
    };
    presentation
        .roster
        .iter()
        .filter_map(
            |(&runtime_id, spans)| match &authority.actor(runtime_id)?.kind {
                protocol::ActorKind::Player { uuid, .. } => Some((*uuid, Arc::clone(spans))),
                protocol::ActorKind::Entity { .. } => None,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::mod_presentation::{Span, SpanPosition};

    /// Spawns one actor of the given kind through the ordered session stream.
    fn spawn(stream: &mut chunk_pipeline::WorldStream, sequence: u64, kind: protocol::ActorKind) {
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(
                    protocol::ActorSpawnEvent {
                        dimension: 0,
                        unique_id: sequence as i64 + 1,
                        runtime_id: sequence + 1,
                        kind,
                        position: [0.; 3],
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

    /// Player runtime IDs map to their roster UUID; mobs and unknown actors draw nothing.
    #[test]
    fn roster_spans_follow_player_runtime_ids_to_their_uuid() {
        let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        spawn(
            &mut stream,
            1,
            protocol::ActorKind::Player {
                uuid: [7; 16],
                username: "Fixture".into(),
            },
        );
        spawn(
            &mut stream,
            2,
            protocol::ActorKind::Entity {
                identifier: "minecraft:zombie".into(),
            },
        );
        let spans = Arc::new(Spans {
            spans: [Span {
                text: "3".into(),
                color: [1.; 4],
            }]
            .into(),
            position: SpanPosition::After,
        });
        let presentation = Presentation {
            roster: [2, 3, 9]
                .into_iter()
                .map(|runtime_id| (runtime_id, Arc::clone(&spans)))
                .collect(),
            ..Default::default()
        };
        let mapped = annotations(&presentation, Some(&stream));
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[&[7; 16]], spans);
        assert!(annotations(&presentation, None).is_empty());
    }
}
