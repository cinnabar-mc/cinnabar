use super::*;

/// A location received for a legacy player marker.
fn player(position: Option<[f32; 3]>) -> LocatorEvent {
    LocatorEvent::Player {
        actor_unique_id: 7,
        position,
        dimension: 0,
    }
}

#[test]
fn locator_hiding_and_experience_deadline_preserve_server_locations() {
    let mut state = LocatorState::default();
    assert!(!state.visible(0, 0));
    state.apply(player(Some([1.0, 64.0, 2.0])));
    assert!(state.visible(0, 0));
    state.experience_changed(100);
    let deadline = 100 + EXPERIENCE_HOLD_MILLIS;
    assert!(!state.visible(0, deadline - 1));
    assert!(state.visible(0, deadline));
    state.apply(player(None));
    assert!(!state.visible(0, deadline));
}

#[test]
fn locator_waypoint_partial_updates_removal_and_dimension_filtering() {
    let mut state = LocatorState::default();
    let id = uuid::Uuid::from_u128(1);
    let event = |update| LocatorEvent::Waypoint {
        id,
        replace: false,
        update: Some(update),
    };
    state.apply(event(LocatorWaypointUpdate {
        position: Some(([1.0, 64.0, 2.0], 1)),
        visible: Some(true),
        ..Default::default()
    }));
    assert!(!state.visible(0, 0));
    assert!(state.visible(1, 0));
    state.apply(event(LocatorWaypointUpdate {
        color: Some([1, 2, 3, 255]),
        ..Default::default()
    }));
    let paint = state.paint(LocatorView::default(), 1);
    assert_eq!(paint.markers.len(), 1);
    assert_eq!(paint.markers[0].color, [1, 2, 3, 255]);
    assert_eq!(paint.markers[0].position, [1.0, 64.0, 2.0]);
    state.apply(event(LocatorWaypointUpdate {
        visible: Some(false),
        ..Default::default()
    }));
    assert!(!state.visible(1, 0));
    state.apply(LocatorEvent::Waypoint {
        id,
        replace: false,
        update: None,
    });
    assert!(state.waypoints.is_empty());
}

#[test]
fn locator_invalid_position_is_counted_without_replacing_the_previous_location() {
    let mut state = LocatorState::default();
    state.apply(player(Some([1.0, 2.0, 3.0])));
    state.apply(player(Some([f32::NAN, 2.0, 3.0])));
    assert_eq!(state.skipped, 1);
    assert_eq!(
        state.paint(LocatorView::default(), 0).markers[0].position,
        [1.0, 2.0, 3.0]
    );
}

#[test]
fn locator_player_locations_and_session_replacement_respect_their_owners() {
    let mut runtime = crate::ui_runtime::UiRuntime::new(1);
    runtime.session.locator.apply(player(Some([1.0, 2.0, 3.0])));
    assert!(runtime.session.locator.visible(0, 0));
    assert!(!runtime.session.locator.visible(1, 0));
    assert!(
        runtime
            .session
            .locator
            .paint(LocatorView::default(), 1)
            .markers
            .is_empty()
    );
    runtime.session = runtime.session.next_session(2);
    assert!(!runtime.session.locator.visible(0, 0));
}

#[test]
fn locator_add_replaces_fields_from_an_earlier_waypoint_with_the_same_id() {
    let mut state = LocatorState::default();
    let id = uuid::Uuid::from_u128(1);
    state.apply(LocatorEvent::Waypoint {
        id,
        replace: false,
        update: Some(LocatorWaypointUpdate {
            position: Some(([1.0, 2.0, 3.0], 0)),
            color: Some([1, 2, 3, 255]),
            ..Default::default()
        }),
    });
    state.apply(LocatorEvent::Waypoint {
        id,
        replace: true,
        update: Some(LocatorWaypointUpdate {
            position: Some(([4.0, 5.0, 6.0], 0)),
            ..Default::default()
        }),
    });
    let paint = state.paint(LocatorView::default(), 0);
    assert_eq!(paint.markers[0].position, [4.0, 5.0, 6.0]);
    assert_eq!(paint.markers[0].color, [255; 4]);
}

#[test]
fn locator_tracks_only_explicitly_authorized_actors_and_honors_hiding() {
    let mut state = LocatorState::default();
    let id = uuid::Uuid::from_u128(1);
    state.apply(LocatorEvent::Waypoint {
        id,
        replace: true,
        update: Some(LocatorWaypointUpdate {
            actor_unique_id: Some(9),
            client_position_authority: Some(true),
            ..Default::default()
        }),
    });
    let mut requested = Vec::new();
    state.observe_actors(1, |actor| {
        requested.push(actor);
        Some([4.0, 5.0, 6.0])
    });
    assert_eq!(requested, vec![9]);
    assert!(!state.visible(0, 0));
    assert!(state.visible(1, 0));
    assert_eq!(
        state.paint(LocatorView::default(), 1).markers[0].position,
        [4.0, 5.0, 6.0]
    );
    state.apply(LocatorEvent::Waypoint {
        id,
        replace: false,
        update: Some(LocatorWaypointUpdate {
            visible: Some(false),
            ..Default::default()
        }),
    });
    assert!(!state.visible(1, 0));
    assert!(state.paint(LocatorView::default(), 1).markers.is_empty());
}
