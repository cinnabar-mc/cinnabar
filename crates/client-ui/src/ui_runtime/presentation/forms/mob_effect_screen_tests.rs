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

/// A runtime whose local player, runtime id 1, has one infinite Speed effect.
fn runtime_with_infinite_speed() -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_effect(
            1,
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
                tick: 0,
            },
            0,
        )
        .unwrap();
    runtime
}

/// Touch follows both renderers' actual artwork, retained paints, safe areas and hidden HUDs.
#[test]
fn effects_touch_target_follows_painted_art_and_retires_with_the_hud() {
    use ui::{SafeArea, UiPoint};
    let build_at = |player: &player_state::PlayerState,
                    presentation: &mut UiPresentationRuntime,
                    runtime: &UiRuntime,
                    now,
                    size,
                    dpi| {
        presentation
            .build(player, runtime, now, size, ui::DpiScale::new(dpi).unwrap())
            .unwrap()
    };
    for renderer in [
        ui_screens::hud_renderers::JAVA_MOB_EFFECTS_RENDERER,
        ui_screens::hud_renderers::MOB_EFFECTS_RENDERER,
    ] {
        let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
            eprintln!("skipping effects touch geometry: missing JSON-UI carrier (make assets)");
            return;
        };
        let player = player_state::PlayerState::new(1);
        let runtime = runtime_with_infinite_speed();
        presentation.hud_frame_mut().first_person = true;
        presentation.set_safe_area(SafeArea::new(23.0, 17.0, 11.0, 9.0).unwrap());
        let screen = serde_json::json!({
            "namespace": "hud",
            "hud_screen": {"type": "screen", "controls": [{"effects": {
                "type": "custom", "renderer": renderer,
                "size": ["100%", "75%"], "offset": [-12, 24]
            }}]}
        });
        presentation.set_server_ui_pack(&super::super::ServerUiPack {
            ui_layers: vec![vec![
                (
                    "ui/_ui_defs.json".into(),
                    br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
                ),
                (
                    "ui/hud_screen.json".into(),
                    serde_json::to_vec(&screen).unwrap(),
                ),
            ]],
            ..Default::default()
        });
        for dpi in [1.0, 2.0] {
            let input = build_at(&player, &mut presentation, &runtime, 0, [1280, 720], dpi);
            let [left, top, right, bottom] = presentation
                .screens
                .hud
                .effect_bounds()
                .expect("effect artwork produces a hit area");
            let center =
                UiPoint::new((left + right) / 2.0 + 23.0, (top + bottom) / 2.0 + 17.0).unwrap();
            assert!(
                presentation.mob_effect_hud_hit(center, [1280.0 / dpi, 720.0 / dpi]),
                "{renderer} at DPI {dpi}"
            );
            assert!(!presentation.mob_effect_hud_hit(
                UiPoint::new(left + 22.0, center.y()).unwrap(),
                [1280.0 / dpi, 720.0 / dpi]
            ));
            let paints = presentation.hud_paints();
            build_at(&player, &mut presentation, &runtime, 0, [1280, 720], dpi);
            assert_eq!(
                presentation.hud_paints(),
                paints,
                "unchanged HUD reuses its paint"
            );
            assert!(
                presentation.mob_effect_hud_hit(center, [1280.0 / dpi, 720.0 / dpi]),
                "reuse retains the hit area"
            );
            super::super::snapshot::write(&input, &format!("effects_touch_{renderer}_{dpi}"));
            presentation.hud_frame_mut().death_screen = true;
            build_at(&player, &mut presentation, &runtime, 0, [1280, 720], dpi);
            assert!(
                !presentation.mob_effect_hud_hit(center, [1280.0 / dpi, 720.0 / dpi]),
                "hidden HUD cannot take touches"
            );
            presentation.hud_frame_mut().death_screen = false;
        }
    }
}

/// A foreground mod button or utility overlay keeps a touch over the effect artwork.
#[test]
fn effect_touches_yield_to_foreground_controls() {
    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        eprintln!("skipping effects touch ownership: missing JSON-UI carrier (make assets)");
        return;
    };
    presentation.enable_touch_controls(true);
    let player = player_state::PlayerState::new(1);
    let runtime = runtime_with_infinite_speed();
    let render = |presentation: &mut UiPresentationRuntime| {
        presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    };
    let window = [1280.0, 720.0];
    render(&mut presentation);
    let point = (0..720)
        .step_by(4)
        .find_map(|y| {
            (0..1280)
                .step_by(4)
                .map(|x| UiPoint::new(x as f32, y as f32).unwrap())
                .find(|point| presentation.mob_effect_hud_hit(*point, window))
        })
        .expect("the active effect has a reachable touch target");
    let position = [point.x() / window[0], point.y() / window[1]];
    assert_eq!(
        presentation
            .gameplay_touch_region(position, window)
            .unwrap()
            .hit_id,
        semantic_input::touch::LOOK_SURFACE
    );
    let button = ui::mod_touch::TouchButton {
        id: "overlap".into(),
        label: "Menu".into(),
        anchor: ui::mod_hud::Anchor::TopRight,
        offset: [2.0; 2],
        size: [48.0; 2],
    };
    presentation.set_mod_touch_buttons([(1, &button)].into_iter());
    render(&mut presentation);
    assert_eq!(
        presentation
            .gameplay_touch_region(position, window)
            .unwrap()
            .hit_id,
        semantic_input::touch::mod_button(0).unwrap()
    );
    assert!(!presentation.mob_effect_hud_hit(point, window));
    presentation.set_mod_touch_buttons(std::iter::empty());
    render(&mut presentation);
    assert!(presentation.mob_effect_hud_hit(point, window));
    presentation.toggle_touch_utilities();
    render(&mut presentation);
    assert!(
        presentation
            .gameplay_touch_region(position, window)
            .is_some_and(|region| region.hit_id != semantic_input::touch::LOOK_SURFACE)
    );
    assert!(!presentation.mob_effect_hud_hit(point, window));
}
