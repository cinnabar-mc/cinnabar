use std::sync::Arc;

use ui::{IconRef, UiNode, UiNodeId, UiVisual};

use super::super::super::player_preview::PreviewView;
use super::tests::{player_catalog_geometry, player_stream};
use super::*;

/// Original geometry whose body, head and limbs differ from the standard biped preview.
fn session_player() -> chunk_pipeline::WorldStream {
    let catalog = player_catalog_geometry(
        br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},"bones":[
        {"name":"root","pivot":[0,0,0]},
        {"name":"body","parent":"root","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
        {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
        {"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}]},
        {"name":"leftLeg","parent":"root","pivot":[2,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[16,48]}]}]}]}"#
            .to_vec(),
    );
    player_stream(Arc::clone(&catalog), catalog)
}

#[test]
fn hud_doll_keeps_its_live_pose_while_the_inventory_shows_its_own_doll() {
    let stream = session_player();
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    presentation.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    let preview = IconRef {
        color_mask: None,
        page: u16::MAX,
        uv: [0, 0, 96, 112],
        glint: false,
    };
    presentation.player_preview_icon = Some(preview);
    presentation.capture_hud_player_reference(Some(stream.authority().local_player_ui()), false);
    // The HUD paints its doll first; the inventory's live renderer paints after it.
    presentation.hud_preview_view = PreviewView::Hud;
    presentation.player_preview_view = PreviewView::Live {
        offset: [40.0, -25.0],
    };
    let bounds = crate::ui_runtime::presentation::rect(0.0, 0.0, 96.0, 112.0).unwrap();
    let placeholder = |id| {
        UiNode::new(UiNodeId::new(id), None, bounds).with_visual(UiVisual::Sprite {
            texture_page: preview.page,
            uv: preview.uv,
            color: [255; 4],
        })
    };
    let mut nodes = vec![placeholder(1), placeholder(2)];
    presentation.apply_gui_models_in(
        &mut nodes,
        bounds,
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
        0..1,
    );
    let (
        UiVisual::TintedMesh { mesh: hud, .. },
        UiVisual::TintedMesh {
            mesh: inventory, ..
        },
    ) = (nodes[0].visual(), nodes[1].visual())
    else {
        panic!("both dolls draw the live actor's palette");
    };
    assert!(hud.is_skinned() && inventory.is_skinned());
    let draw = ui::RetainedDraw::build(
        &nodes,
        bounds,
        ui::UiScale::default(),
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
    )
    .unwrap();
    let poses = super::super::hud_model::model_poses(
        &presentation.gui_models.hud_model,
        &presentation.gui_models.screen_model,
        &draw,
        &nodes,
        1.0,
        [96, 112],
    );
    assert_eq!(poses.len(), 2);
    assert_ne!(
        poses[0].bones, poses[1].bones,
        "each doll draws with its own view"
    );
    let expected = |view| {
        presentation
            .project_gui_player_mesh(
                &presentation.gui_player_sources(view).unwrap(),
                [None, None],
                view,
            )
            .unwrap()
    };
    let projected = presentation
        .gui_models
        .hud_model
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .reference_vertices(hud);
    let expected_hud = expected(PreviewView::Hud);
    assert_eq!(projected.len(), expected_hud.vertices().len());
    for (actual, expected) in projected.iter().zip(expected_hud.vertices()) {
        assert!(
            actual
                .position
                .iter()
                .zip(expected.position)
                .all(|(a, b)| (*a - b).abs() < 1e-5),
            "the HUD must retain its live pose beside the inventory"
        );
        assert!(
            actual
                .uv
                .iter()
                .zip(expected.uv)
                .all(|(a, b)| (*a - b).abs() < 1e-4)
        );
        assert!((actual.model_light - expected.model_light).abs() < 1e-5);
    }
}

/// A presentation holding a captured actor, with one control showing the player preview.
fn doll(view: PreviewView) -> (UiPresentationRuntime, Vec<UiNode>, ui::UiRect) {
    let stream = session_player();
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    presentation.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    let preview = IconRef {
        color_mask: None,
        page: u16::MAX,
        uv: [0, 0, 96, 112],
        glint: false,
    };
    presentation.player_preview_icon = Some(preview);
    presentation.capture_hud_player_reference(Some(stream.authority().local_player_ui()), false);
    presentation.hud_preview_view = view;
    presentation.player_preview_view = view;
    let bounds = crate::ui_runtime::presentation::rect(0.0, 0.0, 96.0, 112.0).unwrap();
    let node = UiNode::new(UiNodeId::new(1), None, bounds).with_visual(UiVisual::Sprite {
        texture_page: preview.page,
        uv: preview.uv,
        color: [255; 4],
    });
    (presentation, vec![node], bounds)
}

/// The on-screen width of the head's top face, in the live renderer's projection.
fn head_top_width(offset: [f32; 2]) -> f32 {
    let (presentation, mut nodes, bounds) = doll(PreviewView::Live { offset });
    presentation.apply_gui_models_in(
        &mut nodes,
        bounds,
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
        1..1,
    );
    let UiVisual::TintedMesh { mesh, .. } = nodes[0].visual() else {
        panic!("the live renderer draws the actor's palette");
    };
    let vertices = presentation
        .gui_models
        .screen_model
        .lock()
        .unwrap()
        .reference_vertices(mesh);
    // With no vertical offset the head's top face holds the highest vertices on screen.
    let top = vertices
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::INFINITY, f32::min);
    let face: Vec<f32> = vertices
        .iter()
        .filter(|vertex| vertex.position[1] < top + 1e-4)
        .map(|vertex| vertex.position[0])
        .collect();
    assert!(!face.is_empty());
    face.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        - face.iter().copied().fold(f32::INFINITY, f32::min)
}

/// Native turns the head twice as far as the body: `atan(dx / 40) * 40` against `* 20`.
#[test]
fn the_live_renderer_turns_the_head_past_the_body_toward_the_pointer() {
    let facing = head_top_width([0.0, 0.0]);
    let turned = head_top_width([40.0, 0.0]);
    let degrees = (1.0_f32).atan() * 40.0;
    let expected = degrees.to_radians().cos() + degrees.to_radians().sin();
    assert!(
        (turned / facing - expected).abs() < 0.02,
        "a head turned {degrees} degrees widens {expected} times, got {}",
        turned / facing
    );
}

/// Outside a world, or before the first capture, the live renderer poses the menu model.
#[test]
fn the_live_renderer_keeps_the_menu_model_without_a_fresh_capture() {
    let (mut presentation, mut nodes, bounds) = doll(PreviewView::Live { offset: [0.0; 2] });
    presentation.begin_live_capture();
    presentation.apply_gui_models_in(
        &mut nodes,
        bounds,
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
        1..1,
    );
    let UiVisual::Mesh(mesh) = nodes[0].visual() else {
        panic!("the menu model is not a palette mesh");
    };
    assert!(!mesh.is_skinned());
    assert!(presentation.live_renderer_shown());
}

/// A non-uniformly scaled model shears its rotated head, which the turn never decomposes.
#[test]
fn the_head_turn_holds_under_non_uniform_model_scale() {
    let facing = Affine3A::from_scale(Vec3::new(-1.0, 1.0, -1.0));
    let frame = Affine3A::from_scale(Vec3::new(2.0, 1.0, 1.0)) * facing;
    let pivot = Vec3::new(0.0, 24.0, 0.0);
    let rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
    let live = LivePlayer {
        head: Some(CapturedHead {
            frame,
            facing: Mat3::from(facing.matrix3),
            pivot,
            rotation,
        }),
        ..Default::default()
    };
    let posed = frame * Affine3A::from_rotation_translation(rotation, pivot);
    let turned = live.head_turn(0.0, 0.0).unwrap() * posed;
    let ahead = frame * Affine3A::from_translation(pivot);
    assert!(turned.abs_diff_eq(ahead, 1e-4), "{turned:?} != {ahead:?}");
}
