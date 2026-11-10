use super::*;
use bevy::prelude::Transform;
use client_presentation::camera::{CameraSettingsAuthority, ServerCameraView, ViewContext};
use client_ui::test_support::fixture_hud;
use client_ui::ui_runtime::presentation::refresh_hud_frame;
use protocol::{CameraEvent, CameraInstructionEvent, CameraPreset, CameraSetInstruction};
use semantic_input::PerspectiveMode;
use ui_engine::test_support::fixture_font;

#[test]
fn free_camera_retires_cpu_hand_draws_and_clear_restores_them() {
    let player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = UiRuntime::new(1);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.set_player_preview_skin(Some(&vec![255; 64 * 64 * 4]), Default::default());
    refresh_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        None,
        PerspectiveMode::FirstPerson,
        0,
    );
    let mut camera = ServerCameraView::default();
    let context = ViewContext {
        base: Transform::IDENTITY,
        subject: Transform::IDENTITY,
        base_fov: 70.0,
        actors: &|_| None,
    };
    camera.apply(
        1,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: "minecraft:free".into(),
                ..Default::default()
            }]
            .into(),
        ),
        &context,
    );
    for (sequence, instruction, expected) in [
        (2, CameraInstructionEvent::default(), true),
        (
            3,
            CameraInstructionEvent {
                set: Some(CameraSetInstruction {
                    preset_id: 0,
                    ease: None,
                    position: None,
                    rotation_degrees: None,
                    facing_position: None,
                    view_offset: None,
                    entity_offset: None,
                    default_preset: None,
                    remove_ignore_starting_values: false,
                }),
                ..Default::default()
            },
            false,
        ),
        (
            4,
            CameraInstructionEvent {
                clear: Some(true),
                ..Default::default()
            },
            true,
        ),
    ] {
        camera.apply(
            sequence,
            &CameraEvent::Instruction(Box::new(instruction)),
            &context,
        );
        let mut settings = CameraSettingsAuthority::default();
        settings.reset_perspective();
        presentation.hud_frame_mut().first_person =
            client_presentation::camera::hand_first_person(Some(&settings), Some(&camera));
        assert_eq!(
            draws_cpu_hand(&mut presentation, &player, &runtime),
            expected,
            "camera instruction {sequence}"
        );
    }
}

#[test]
fn scripted_camera_retires_cpu_hand_draws_and_release_restores_them() {
    let player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = UiRuntime::new(1);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.set_player_preview_skin(Some(&vec![255; 64 * 64 * 4]), Default::default());
    refresh_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        None,
        PerspectiveMode::FirstPerson,
        0,
    );
    let mut settings = CameraSettingsAuthority::default();
    settings.reset_perspective();
    for (scripted, expected) in [(false, true), (true, false), (false, true)] {
        settings.set_scripted_view(scripted);
        presentation.hud_frame_mut().first_person =
            client_presentation::camera::hand_first_person(Some(&settings), None);
        assert_eq!(
            draws_cpu_hand(&mut presentation, &player, &runtime),
            expected,
            "scripted camera {scripted}"
        );
    }
}

/// Builds the HUD and reports whether any batch samples the CPU right-hand carrier.
fn draws_cpu_hand(
    presentation: &mut UiPresentationRuntime,
    player: &client_state::PlayerRuntime,
    runtime: &UiRuntime,
) -> bool {
    let hand = presentation.hud_frame().right_hand.expect("hand carrier");
    let input = presentation
        .build(player, runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
        .unwrap();
    input.batches.iter().any(|batch| {
        batch.texture_page == u32::from(hand.page)
            && input.indices
                [batch.first_index as usize..(batch.first_index + batch.index_count) as usize]
                .iter()
                .any(|&index| {
                    input.vertices[index as usize].uv
                        == [f32::from(hand.uv[0]), f32::from(hand.uv[1])]
                })
    })
}
