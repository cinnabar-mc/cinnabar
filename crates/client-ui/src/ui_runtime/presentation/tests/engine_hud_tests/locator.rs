//! Server locator updates must reach visible artwork in both gameplay modes.

use super::*;
use protocol::{LocatorEvent, LocatorWaypointUpdate};
use ui_screens::hud_renderers::{LOCATOR_RENDERER, LocatorView};

#[test]
fn locator_bar_paints_server_markers_in_survival_and_creative() {
    for mode in [PlayerGameMode::Survival, PlayerGameMode::Creative] {
        let Some(mut presentation) =
            engine_presentation_with(super::super::super::forms::pack_harness::font())
        else {
            eprintln!(
                "skipping locator_bar_paints_server_markers_in_survival_and_creative: missing UI carrier; run make assets"
            );
            return;
        };
        let mut player = player_state::PlayerState::new(1);
        player.facts.publish_player_game_mode(mode);
        let mut runtime = UiRuntime::new(1);
        select_slot(&mut player, &mut runtime);
        full_stats(&mut player, &mut runtime, 1);
        runtime.session.hud.set_experience(2, 0.4);
        presentation.hud_frame_mut().locator_view = Some(LocatorView {
            position: [0.0; 3],
            forward: [0.0, 0.0, 1.0],
            vertical_fov: std::f32::consts::PI / 3.0,
            ..Default::default()
        });
        let before = build(&player, &mut presentation, &runtime, 0);
        super::super::super::forms::snapshot::write(
            &before,
            &format!("locator-without-locations-{mode:?}"),
        );
        assert!(customs(presentation.hud_draw_nodes(), LOCATOR_RENDERER).is_empty());
        let color = [12, 200, 55, 255];
        let id = uuid::Uuid::from_u128(1);
        runtime
            .apply(
                &mut player,
                crate::ui_runtime::SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 2,
                    local_millis: 20,
                    server_tick: None,
                    event: protocol::UiEvent::Locator(vec![LocatorEvent::Waypoint {
                        id,
                        replace: false,
                        update: Some(LocatorWaypointUpdate {
                            visible: Some(true),
                            position: Some(([0.0, 0.0, 10.0], 0)),
                            color: Some(color),
                            ..Default::default()
                        }),
                    }]),
                },
            )
            .unwrap();
        let input = build(&player, &mut presentation, &runtime, 20);
        super::super::super::forms::snapshot::write(&input, &format!("locator-{mode:?}"));
        assert_eq!(
            customs(presentation.hud_draw_nodes(), LOCATOR_RENDERER).len(),
            1
        );
        assert!(
            input.vertices.iter().any(|vertex| vertex.color == color),
            "locator control emitted no marker artwork in {mode:?}"
        );
        assert!(named(presentation.hud_draw_nodes(), "empty_progress_bar").is_empty());
        let original_left = input
            .vertices
            .iter()
            .filter(|vertex| vertex.color == color)
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        runtime
            .apply_local_attributes(
                &mut player,
                crate::ui_runtime::SequencedLocalAttributes {
                    session_id: 1,
                    fifo_sequence: 3,
                    local_millis: 30,
                    server_tick: 2,
                    attributes: vec![protocol::ActorAttribute {
                        name: Arc::from("minecraft:player.experience"),
                        min: 0.0,
                        max: 1.0,
                        current: 0.5,
                        default: None,
                        modifiers: Arc::from([]),
                    }]
                    .into(),
                },
            )
            .unwrap();
        let deadline = 30 + crate::ui_runtime::locator::EXPERIENCE_HOLD_MILLIS;
        for now in [30, deadline - 1] {
            build(&player, &mut presentation, &runtime, now);
            assert!(customs(presentation.hud_draw_nodes(), LOCATOR_RENDERER).is_empty());
        }
        build(&player, &mut presentation, &runtime, deadline);
        assert_eq!(
            customs(presentation.hud_draw_nodes(), LOCATOR_RENDERER).len(),
            1
        );
        presentation.set_server_ui_pack(&super::super::super::ServerUiPack {
            ui_layers: vec![vec![("ui/hud_screen.json".into(), br##"{
                "namespace":"hud", "centered_gui_elements_at_bottom_middle": {
                    "controls":[{"server_locator@hud.locator_bar":{
                        "size":[80,5], "offset":[200,-60],
                        "bindings":[{"binding_name":"#hotbar_with_locator_bar","binding_name_override":"#visible"}]
                    }}]
                }
            }"##.to_vec())]], ..Default::default()
        });
        let moved = build(&player, &mut presentation, &runtime, deadline + 5);
        let moved_left = moved
            .vertices
            .iter()
            .filter(|vertex| vertex.color == color)
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        assert!(
            moved_left.is_finite() && (moved_left - original_left).abs() > 1.0,
            "server UI must move the marker artwork with its locator control"
        );
        runtime.session.locator.apply(LocatorEvent::Waypoint {
            id,
            replace: false,
            update: None,
        });
        build(&player, &mut presentation, &runtime, deadline + 10);
        assert!(customs(presentation.hud_draw_nodes(), LOCATOR_RENDERER).is_empty());
    }
}
