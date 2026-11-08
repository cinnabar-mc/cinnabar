//! Server-authoritative locator updates, independent of the wire representation.

use std::sync::Arc;

use bedrock_protocol_1_26_60::{enums::ServerWaypointGroupAction, types};

/// A player location or a partial update to a server waypoint.
#[derive(Clone, Debug, PartialEq)]
pub enum LocatorEvent {
    Player {
        actor_unique_id: i64,
        position: Option<[f32; 3]>,
        dimension: i32,
    },
    Waypoint {
        id: uuid::Uuid,
        replace: bool,
        update: Option<LocatorWaypointUpdate>,
    },
}

/// Fields absent from an update retain their previous values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocatorWaypointUpdate {
    /// Well-framed fields omitted because their values cannot be used.
    pub skipped_fields: u32,
    pub visible: Option<bool>,
    pub position: Option<([f32; 3], i32)>,
    pub texture: Option<Arc<str>>,
    pub size: Option<[f32; 2]>,
    pub color: Option<[u8; 4]>,
    pub actor_unique_id: Option<i64>,
    pub client_position_authority: Option<bool>,
}

/// Preserves waypoint removals and partial fields without imposing a display limit.
pub(crate) fn waypoint(value: types::LocatorBarWaypoint) -> Option<LocatorEvent> {
    let id = value.group_handle.uuid;
    let replace = value.action_flag == ServerWaypointGroupAction::Add;
    let update = match value.action_flag {
        ServerWaypointGroupAction::Remove => None,
        ServerWaypointGroupAction::Add | ServerWaypointGroupAction::Update => {
            let data = value.server_waypoint_payload;
            let mut skipped_fields = 0;
            let texture = data.texture_path.and_then(|path| match path.to_str() {
                Ok(path) => Some(Arc::from(path)),
                Err(_) => {
                    skipped_fields += 1;
                    None
                }
            });
            Some(LocatorWaypointUpdate {
                skipped_fields,
                visible: data.is_visible,
                position: data
                    .world_position
                    .map(|p| (p.position.to_array(), p.dimension_type.0)),
                texture,
                size: data.icon_size.map(|size| size.to_array()),
                color: data.color.map(|color| {
                    let value = color.0 as u32;
                    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
                }),
                actor_unique_id: data.actor_unique_id.map(|id| id.0),
                client_position_authority: data.client_position_authority,
            })
        }
        _ => return None,
    };
    Some(LocatorEvent::Waypoint {
        id,
        replace,
        update,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Packet, UiEvent, WorldEvent, into_world_event};
    use bedrock_protocol_1_26_60::{enums::PlayerLocationType, packets, wire};

    #[test]
    fn locator_player_packets_reach_ui_including_explicit_hiding() {
        for position in [Some([1.0, 64.0, -3.0]), None] {
            let location = match position {
                Some(position) => types::PlayerLocationData::CoordinatesLocation {
                    packet_type: PlayerLocationType::PlayerLocationCoordinates,
                    position: position.into(),
                },
                None => types::PlayerLocationData::HiddenLocation {
                    packet_type: PlayerLocationType::PlayerLocationHide,
                },
            };
            let packet = Packet::new(packets::PlayerLocation {
                target_actor_id: types::ActorUniqueID(-7),
                location,
            });
            assert_eq!(
                into_world_event(packet, 0).unwrap(),
                Some(WorldEvent::Ui(UiEvent::Locator(vec![
                    LocatorEvent::Player {
                        actor_unique_id: -7,
                        position,
                        dimension: 0,
                    }
                ])))
            );
        }
    }

    #[test]
    fn locator_waypoint_packets_preserve_partial_updates_and_removals() {
        let id = uuid::Uuid::from_u128(1);
        let entry = |action| types::LocatorBarWaypoint {
            group_handle: types::WaypointGroupWaypointHandle { uuid: id },
            action_flag: action,
            server_waypoint_payload: types::ServerWaypoint {
                is_visible: Some(true),
                texture_path: Some("textures/ui/test_waypoint".into()),
                color: Some(types::MceColor(0x123456)),
                update_flag: wire::U32LE(0),
                ..Default::default()
            },
        };
        let packet = Packet::new(packets::LocatorBar {
            waypoints: vec![
                entry(ServerWaypointGroupAction::Update),
                entry(ServerWaypointGroupAction::Remove),
            ],
        });
        let Some(WorldEvent::Ui(UiEvent::Locator(events))) = into_world_event(packet, 0).unwrap()
        else {
            panic!("locator packet was dropped");
        };
        assert_eq!(events.len(), 2);
        let LocatorEvent::Waypoint {
            update: Some(update),
            ..
        } = &events[0]
        else {
            panic!("missing update");
        };
        assert_eq!(update.position, None);
        assert_eq!(update.visible, Some(true));
        assert_eq!(update.color, Some([0x12, 0x34, 0x56, 255]));
        assert_eq!(
            events[1],
            LocatorEvent::Waypoint {
                id,
                replace: false,
                update: None
            }
        );
    }
}
