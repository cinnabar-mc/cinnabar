//! The vanilla pack's mob effect screen lists the local player's effects with a close button.

use protocol::{ActorEffectAction, ActorEffectEvent};
use ui::{UiNode, UiPoint};
use ui_engine::text_metrics::TextMetrics;

use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::UiPresentationRuntime;

fn draw(presentation: &mut UiPresentationRuntime, runtime: &UiRuntime) -> Vec<UiNode> {
    let mut nodes = Vec::new();
    let mut next = 0;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), None);
    presentation
        .append_mob_effect_screen(runtime, &mut nodes, &mut next, metrics, [1280.0, 720.0], 0)
        .unwrap();
    nodes
}

#[test]
fn pack_screen_lists_named_effects_with_timers_and_a_close_button() {
    let mut runtime = super::super::pack_harness::menu_runtime();
    let presentation = super::super::pack_harness::engine_presentation();
    let (Some(mut presentation), Some(_)) = (presentation, runtime.translation("potion.moveSpeed"))
    else {
        eprintln!(
            "skipping mob effect screen test: missing fixture JSON-UI carrier or vanilla-v1.mcbelang"
        );
        return;
    };
    for (fifo, effect_id, amplifier, duration_ticks) in [(1, 1, 1, 20 * 120), (2, 16, 0, -1)] {
        runtime
            .apply_local_effect(
                runtime.session_id(),
                fifo,
                ActorEffectEvent {
                    dimension: 0,
                    actor_runtime_id: 1,
                    action: ActorEffectAction::Add,
                    effect_id,
                    amplifier,
                    particles: true,
                    ambient: false,
                    duration_ticks,
                    tick: 100,
                },
                0,
            )
            .unwrap();
    }
    assert!(runtime.toggle_mob_effect_screen(0));
    let nodes = draw(&mut presentation, &runtime);
    let texts = super::super::pack_harness::drawn_texts(&nodes);
    let speed = format!(
        "{} {}",
        runtime.translation("potion.moveSpeed").unwrap(),
        runtime.translation("potion.potency.1").unwrap()
    );
    for expected in [
        speed.as_str(),
        "2:00",
        &runtime.translation("potion.nightVision").unwrap(),
        &runtime.translation("effect.duration.infinite").unwrap(),
    ] {
        assert!(
            texts.iter().any(|text| text == expected),
            "{expected:?} missing from {texts:?}"
        );
    }
    // Wrapped labels drop their break spaces, so compare without whitespace.
    let compact = |text: &str| text.split_whitespace().collect::<String>();
    let bundle = compact(
        &runtime
            .translation("item.bundle.empty.description")
            .unwrap(),
    );
    assert!(
        !texts.iter().any(|text| compact(text) == bundle),
        "the base screen's container-only bundle tooltip is drawn: {texts:?}"
    );
    let frame = presentation
        .mob_effect_frame()
        .expect("pack screen produced an engine frame");
    let close = frame
        .hits
        .iter()
        .find(|hit| hit.enabled && hit.pressed.as_deref() == Some("button.menu_continue"))
        .expect("authored close button");
    let point = UiPoint::new(
        frame.origin[0] + (close.rect.x + close.rect.w / 2.0) as f32 * frame.scale,
        frame.origin[1] + (close.rect.y + close.rect.h / 2.0) as f32 * frame.scale,
    )
    .unwrap();
    assert!(presentation.mob_effect_close_hit(point));
    assert!(!presentation.mob_effect_close_hit(UiPoint::new(1.0, 700.0).unwrap()));
}

#[test]
fn a_pack_that_hides_the_game_behind_the_screen_stops_world_rendering() {
    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        eprintln!("skipping mob effect world policy test: missing fixture JSON-UI carrier");
        return;
    };
    let player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_effect(
            runtime.session_id(),
            1,
            ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 1,
                action: ActorEffectAction::Add,
                effect_id: 1,
                amplifier: 0,
                particles: true,
                ambient: false,
                duration_ticks: -1,
                tick: 100,
            },
            0,
        )
        .unwrap();
    assert!(runtime.toggle_mob_effect_screen(0));
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, None));
    presentation.set_server_ui_pack(&super::super::ServerUiPack {
        ui_layers: vec![vec![(
            "ui/mob_effect_screen.json".into(),
            br#"{"namespace":"mob_effect","mob_effect_screen":{"render_game_behind":false}}"#
                .to_vec(),
        )]],
        ..Default::default()
    });
    assert!(!presentation.renders_game_behind(&player_runtime, &runtime, None));
    runtime.close_mob_effect_screen();
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, None));
}
