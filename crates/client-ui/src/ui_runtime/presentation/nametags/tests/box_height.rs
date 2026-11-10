//! Label heights above actor boxes, and zero-scale text carriers clear of the labels below them.

use super::*;

/// The world-Y span a label's records cover.
fn vertical_span(tag: NametagAnchor) -> (f32, f32) {
    let rendered = scene(&[tag]);
    assert!(!rendered.records.is_empty());
    rendered
        .records
        .iter()
        .flat_map(|record| record.world_corners(glam::Quat::IDENTITY).unwrap())
        .fold((f32::MAX, f32::MIN), |(low, high), corner| {
            (low.min(corner[1]), high.max(corner[1]))
        })
}

/// An always-shown label for `actor` with the given feet and text.
fn label(actor: &ActorSnapshot, feet: Vec3, text: &str) -> NametagAnchor {
    extract_nametag_at_feet(actor, view(Vec3::ZERO), None, text.into(), None, feet).unwrap()
}

/// A named non-player actor with metadata scale and, when published, height.
fn entity(scale: f32, height: Option<f32>) -> ActorSnapshot {
    let mut actor = actor();
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:sheep".into(),
    };
    actor
        .metadata
        .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(1));
    actor.metadata.insert(38, ActorMetadataValue::Float(scale));
    match height {
        Some(height) => actor
            .metadata
            .insert(METADATA_HEIGHT, ActorMetadataValue::Float(height)),
        None => actor.metadata.remove(&METADATA_HEIGHT),
    };
    actor
}

#[test]
fn zero_scale_text_above_a_scaled_npc_does_not_cover_its_top_line() {
    let npc = label(
        &entity(1.4, Some(1.85)),
        Vec3::new(0.0, 40.0, -5.0),
        "Top\nMiddle\nBottom",
    );
    let text = label(&entity(0.0, None), Vec3::new(0.0, 43.44, -5.0), "Rotating");
    let (_, npc_top) = vertical_span(npc);
    let (text_bottom, _) = vertical_span(text);
    assert!(
        text_bottom > npc_top,
        "text bottom {text_bottom} overlaps the NPC's top line at {npc_top}"
    );
}

#[test]
fn zero_scale_passenger_text_sits_clear_above_its_host_players_name() {
    let feet = Vec3::new(0.0, 19.0, -5.0);
    let host = label(&entity(1.0, Some(1.8)), feet, "Name");
    // The seat a server-spawned health stand occupies on a player.
    let stand = label(&entity(0.0, Some(0.0)), feet + Vec3::Y * 2.12, "9.5");
    let (_, host_top) = vertical_span(host);
    let (stand_bottom, _) = vertical_span(stand);
    assert!(
        stand_bottom > host_top,
        "passenger text bottom {stand_bottom} overlaps the name top {host_top}"
    );
}

#[test]
fn labels_clear_the_box_by_vanilla_seven_tenths_of_a_block() {
    let mut actor = actor();
    assert!((tag_height(&actor) - 2.5).abs() < 1e-6);
    // A server text carrier with an empty box still hangs its label 0.7 above its feet.
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:armor_stand".into(),
    };
    actor
        .metadata
        .insert(METADATA_HEIGHT, ActorMetadataValue::Float(0.0));
    assert!((tag_height(&actor) - (client_world::MIN_COLLISION_DIMENSION + 0.7)).abs() < 1e-6);
}
