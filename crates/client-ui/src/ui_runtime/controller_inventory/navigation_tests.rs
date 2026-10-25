//! Original synthetic geometry; no game assets or native-timing assumptions.
use super::*;
use json_ui::{FocusMeta, HitKind};

fn bounds() -> RectOut {
    RectOut {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    }
}

fn region(index: usize, x: f64, y: f64) -> HitRegion {
    HitRegion {
        key: format!("panel/{index}"),
        name: format!("cell{index}"),
        kind: HitKind::Button,
        rect: RectOut {
            x,
            y,
            w: 10.0,
            h: 10.0,
        },
        clip: bounds(),
        layer: 0,
        order: index,
        pressed: None,
        control_name: None,
        collection_index: Some(index),
        collection: None,
        enabled: true,
        checked: None,
        max_length: None,
        group_index: None,
        renderer: None,
        drag_axes: [false; 2],
        sound: None,
        input: Default::default(),
        focus: Some(FocusMeta {
            enabled: true,
            precedence: 1,
            wrap: false,
            magnet: true,
            identifier: format!("id{index}"),
            reset_on_focus_lost: true,
            change: Default::default(),
            mapping: Vec::new(),
            containers: Vec::new(),
        }),
        collections: Vec::new(),
        widget: Default::default(),
        modal_root: None,
        transform: None,
    }
}

fn targets(hits: &[HitRegion]) -> Vec<Target> {
    hits.iter()
        .enumerate()
        .map(|(index, hit)| Target {
            key: hit.key.clone(),
            identity: index as u64,
            hit: InventoryCellHit::Player(index as u8),
        })
        .collect()
}

fn navigation() -> Navigation {
    Navigation::new(Settings {
        cursor_units_per_second: 100.0,
        cursor_deadzone: 0.2,
        repeat_delay: Duration::from_millis(300),
        repeat_interval: Duration::from_millis(100),
    })
    .unwrap()
}

fn input(milliseconds: u64, directions: [bool; 4]) -> Input {
    Input {
        now: Duration::from_millis(milliseconds),
        controller: 1,
        active: true,
        directions,
        axes: [0.0; 2],
        mouse: None,
    }
}

fn sample(nav: &mut Navigation, hits: &[HitRegion], targets: &[Target], input: Input) -> Output {
    nav.update(
        Frame {
            generation: 1,
            geometry_revision: 1,
            bounds: bounds(),
            hits,
            targets,
        },
        input,
    )
    .clone()
}

const NEUTRAL: [bool; 4] = [false; 4];
const RIGHT: [bool; 4] = [false, false, false, true];

#[test]
fn open_requires_release_then_press_is_immediate_and_repeat_has_exact_deadlines() {
    let hits: Vec<_> = (0..5)
        .map(|i| region(i, 10.0 + i as f64 * 20.0, 10.0))
        .collect();
    let targets = targets(&hits);
    let mut nav = navigation();
    assert!(
        sample(&mut nav, &hits, &targets, input(0, RIGHT))
            .selection
            .is_none()
    );
    sample(&mut nav, &hits, &targets, input(1, NEUTRAL));
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(2, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        0
    );
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(301, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        0
    );
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(302, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        1
    );
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(502, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        3
    );
    sample(&mut nav, &hits, &targets, input(503, NEUTRAL));
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(504, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        4
    );
}

#[test]
fn repeat_result_is_independent_of_sampling_rate() {
    let hits: Vec<_> = (0..8)
        .map(|i| region(i, 10.0 + i as f64 * 20.0, 10.0))
        .collect();
    let targets = targets(&hits);
    let run = |samples: &[u64]| {
        let mut nav = navigation();
        sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
        sample(&mut nav, &hits, &targets, input(1, RIGHT));
        for &time in samples {
            sample(&mut nav, &hits, &targets, input(time, RIGHT));
        }
        nav.output.selection.unwrap().target.identity
    };
    assert_eq!(run(&[701]), run(&[34, 68, 102, 301, 401, 501, 601, 701]));
    assert_eq!(run(&[701]), 5);
    for rate in [30, 60, 144] {
        let mut times: Vec<_> = (1..)
            .map(|frame| 1 + frame * 1000 / rate)
            .take_while(|time| *time < 701)
            .collect();
        times.push(701);
        assert_eq!(run(&times), 5);
    }
}

#[test]
fn overrides_stop_no_wrap_and_partial_rows_use_json_ui_not_slot_arithmetic() {
    let mut hits = vec![
        region(0, 10.0, 10.0),
        region(1, 50.0, 10.0),
        region(2, 10.0, 40.0),
    ];
    hits[0].focus.as_mut().unwrap().change[3] = "id2".into();
    hits[2].focus.as_mut().unwrap().change[0] = json_ui::FOCUS_OVERRIDE_STOP.into();
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    sample(&mut nav, &hits, &targets, input(2, NEUTRAL));
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(3, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        2
    );
    sample(&mut nav, &hits, &targets, input(4, NEUTRAL));
    assert_eq!(
        sample(
            &mut nav,
            &hits,
            &targets,
            input(5, [true, false, false, false])
        )
        .selection
        .unwrap()
        .target
        .identity,
        2
    );
    sample(&mut nav, &hits, &targets, input(6, NEUTRAL));
    assert_eq!(
        sample(
            &mut nav,
            &hits,
            &targets,
            input(7, [false, true, false, false])
        )
        .selection
        .unwrap()
        .target
        .identity,
        2
    );
}

#[test]
fn disabled_clipped_and_modal_blocked_targets_are_not_admitted() {
    let mut hits = vec![
        region(0, 10.0, 10.0),
        region(1, 30.0, 10.0),
        region(2, 50.0, 10.0),
        region(3, 70.0, 10.0),
    ];
    hits[0].enabled = false;
    hits[1].clip.w = 5.0;
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(1, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        2
    );
    let mut modal = region(4, 0.0, 0.0);
    modal.kind = HitKind::Modal;
    modal.focus = None;
    modal.key = "modal".into();
    hits.push(modal);
    hits[3].modal_root = Some("modal".into());
    assert!(
        sample(&mut nav, &hits, &targets, input(2, NEUTRAL))
            .selection
            .is_none()
    );
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(3, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        3
    );
}

#[test]
fn stable_identity_survives_reflow_but_recycled_recipe_index_does_not() {
    let mut hits = vec![region(0, 10.0, 10.0)];
    let mut targets = targets(&hits);
    targets[0].hit = InventoryCellHit::RecipeBook(0);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    hits[0].key = "new_panel/recipe".into();
    hits[0].rect.x = 80.0;
    targets[0].key = hits[0].key.clone();
    targets[0].hit = InventoryCellHit::RecipeBook(5);
    let output = sample(&mut nav, &hits, &targets, input(2, NEUTRAL));
    assert_eq!(output.cursor, Some([85.0, 15.0]));
    assert_eq!(
        output.selection.unwrap().target.hit,
        InventoryCellHit::RecipeBook(5)
    );
    assert!(output.reveal_key.is_none());
    targets[0].identity = 999;
    assert!(
        sample(&mut nav, &hits, &targets, input(3, NEUTRAL))
            .selection
            .is_none()
    );
}

#[test]
fn cursor_motion_clears_stale_focus_and_never_snaps_on_release() {
    let hits = vec![region(0, 10.0, 10.0), region(1, 30.0, 10.0)];
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    let mut moving = input(201, NEUTRAL);
    moving.axes = [1.0, 0.0];
    let output = sample(&mut nav, &hits, &targets, moving);
    assert_eq!(output.cursor, Some([35.0, 15.0]));
    assert_eq!(output.selection.unwrap().target.identity, 1);
    let mut moving = input(401, NEUTRAL);
    moving.axes = [1.0, 0.0];
    let output = sample(&mut nav, &hits, &targets, moving);
    assert!(output.selection.is_none());
    assert!(output.focused_key.is_none());
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(402, NEUTRAL)).cursor,
        output.cursor
    );
}

#[test]
fn deadzone_diagonals_opposites_and_nonfinite_input_are_deterministic() {
    assert_eq!(velocity([0.1, 0.1], 0.2), [0.0; 2]);
    assert_eq!(velocity([f64::NAN, f64::INFINITY], 0.2), [0.0; 2]);
    let diagonal = velocity([1.0, 1.0], 0.2);
    assert!((diagonal[0].hypot(diagonal[1]) - 1.0).abs() < 1e-12);
    assert_eq!(direction([true, true, true, true]), None);
    assert_eq!(
        direction([true, false, false, true]),
        Some(FocusDirection::Up)
    );
    assert_eq!(
        direction([true, true, false, true]),
        Some(FocusDirection::Right)
    );
}

#[test]
fn ownership_time_reset_and_mouse_takeover_require_neutral() {
    let hits = vec![region(0, 10.0, 10.0)];
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    let mut replaced = input(2, RIGHT);
    replaced.controller = 2;
    assert!(
        sample(&mut nav, &hits, &targets, replaced)
            .selection
            .is_none()
    );
    sample(&mut nav, &hits, &targets, input(3, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(4, RIGHT));
    assert!(
        sample(&mut nav, &hits, &targets, input(1, RIGHT))
            .selection
            .is_none()
    );
    sample(&mut nav, &hits, &targets, input(2, NEUTRAL));
    let mut mouse = input(3, RIGHT);
    mouse.mouse = Some([15.0, 15.0]);
    assert_eq!(sample(&mut nav, &hits, &targets, mouse).mode, Mode::Mouse);
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(4, RIGHT)).mode,
        Mode::Mouse
    );
    nav.reset();
    assert!(
        sample(&mut nav, &hits, &targets, input(5, RIGHT))
            .selection
            .is_none()
    );
}

#[test]
fn generation_change_invalidates_tokens_and_held_repeat() {
    let hits = vec![region(0, 10.0, 10.0)];
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    let output = nav.update(
        Frame {
            generation: 2,
            geometry_revision: 9,
            bounds: bounds(),
            hits: &hits,
            targets: &targets,
        },
        input(1000, RIGHT),
    );
    assert!(output.selection.is_none());
    assert!(output.cursor.is_none());
}

#[test]
fn invalid_settings_fail_without_a_clock_or_runtime() {
    let settings = navigation().settings;
    assert!(
        Navigation::new(Settings {
            repeat_interval: Duration::ZERO,
            ..settings
        })
        .is_err()
    );
    assert!(
        Navigation::new(Settings {
            cursor_deadzone: 1.0,
            ..settings
        })
        .is_err()
    );
    assert!(
        Navigation::new(Settings {
            cursor_units_per_second: f64::NAN,
            ..settings
        })
        .is_err()
    );
}

#[test]
fn invalidation_cancels_repeat_until_release_even_if_a_row_is_recycled() {
    let hits = vec![region(0, 10.0, 10.0), region(1, 30.0, 10.0)];
    let mut targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    targets[0].identity = 100;
    assert!(
        sample(&mut nav, &hits, &targets, input(301, RIGHT))
            .selection
            .is_none()
    );
    assert!(
        sample(&mut nav, &hits, &targets, input(901, RIGHT))
            .selection
            .is_none()
    );
    sample(&mut nav, &hits, &targets, input(902, NEUTRAL));
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(903, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        100
    );
}

#[test]
fn claimed_directions_are_reported_without_moving_focus() {
    let mut hits = vec![region(0, 10.0, 10.0), region(1, 30.0, 10.0)];
    hits[0].input.always_handle_controller_direction = true;
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    let output = sample(&mut nav, &hits, &targets, input(301, RIGHT));
    assert_eq!(
        output.claimed_direction,
        Some((hits[0].key.clone(), FocusDirection::Right))
    );
    assert_eq!(output.selection.unwrap().target.identity, 0);
}

#[test]
fn cursor_integration_is_rate_independent_and_bounds_are_gui_coordinates() {
    let run = |rate: u64| {
        let mut nav = navigation();
        let mut neutral = input(0, NEUTRAL);
        neutral.mouse = Some([10.0, 20.0]);
        sample(&mut nav, &[], &[], neutral);
        sample(&mut nav, &[], &[], input(0, NEUTRAL));
        for frame in 1..=rate {
            let mut moving = input(frame * 1000 / rate, NEUTRAL);
            moving.axes = [1.0, 0.0];
            sample(&mut nav, &[], &[], moving);
        }
        nav.output.cursor.unwrap()
    };
    for rate in [30, 60, 144] {
        let point = run(rate);
        assert!((point[0] - 110.0).abs() < 1e-9);
        assert_eq!(point[1], 20.0);
    }
    assert_eq!(clamp([-10.0, 999.0], bounds()), [0.0, 100.0]);
}

#[test]
fn visible_points_handle_partial_clipping_translation_and_collapsed_transforms() {
    let mut hit = region(0, 10.0, 10.0);
    hit.clip = RectOut {
        x: 12.0,
        y: 12.0,
        w: 2.0,
        h: 2.0,
    };
    assert_eq!(visible_point(&hit), Some([13.0, 13.0]));
    hit.clip = bounds();
    hit.transform = Some(json_ui::transform::Affine {
        linear: [1.0, 0.0, 0.0, 1.0],
        translation: [20.0, 0.0],
    });
    assert_eq!(visible_point(&hit), Some([35.0, 15.0]));
    hit.transform.as_mut().unwrap().linear = [0.0; 4];
    assert_eq!(visible_point(&hit), None);
}

/// The clip intersects a triangle of the diamond, but its bounding-box midpoint misses it.
fn clipped_diamond() -> HitRegion {
    let mut hit = region(0, 0.0, 0.0);
    hit.transform = Some(json_ui::transform::Affine {
        linear: [1.0, 1.0, -1.0, 1.0],
        translation: [0.0; 2],
    });
    hit.clip = RectOut {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 5.0,
    };
    hit
}

#[test]
fn clipped_rotated_polygon_remains_focusable_with_an_interior_cursor() {
    let hit = clipped_diamond();
    assert!(!hit.contains([5.0, 2.5]));
    let anchor = visible_point(&hit).expect("the clipped triangle has positive area");
    assert!(hit.contains(anchor));
    assert!(anchor[0] > 0.0 && anchor[0] < anchor[1] && anchor[1] < 5.0);

    let hits = [hit];
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    let output = sample(&mut nav, &hits, &targets, input(1, RIGHT));
    assert_eq!(
        output.selection.unwrap().target.identity,
        targets[0].identity
    );
    assert_eq!(output.cursor, Some(anchor));
}

#[test]
fn rotated_polygon_wholly_clipped_or_touching_only_the_boundary_has_no_anchor() {
    let mut hit = clipped_diamond();
    // The painted bounding box still overlaps this clip, but the polygon does not.
    hit.clip.x = 6.0;
    hit.clip.w = 4.0;
    assert_eq!(visible_point(&hit), None);
    // Only [5, 5] reaches this clip; the intersection has no visible area.
    hit.clip.x = 5.0;
    hit.clip.w = 5.0;
    assert_eq!(visible_point(&hit), None);

    let hits = [hit];
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    let output = sample(&mut nav, &hits, &targets, input(1, RIGHT));
    assert!(output.selection.is_none());
    assert!(output.cursor.is_none());
}

#[test]
fn translated_clipped_polygon_preserves_anchor_and_invalidates_when_fully_hidden() {
    let mut hit = clipped_diamond();
    let original = visible_point(&hit).unwrap();
    let translation = [40.0, 20.0];
    hit.transform.as_mut().unwrap().translation = translation;
    hit.clip.x += translation[0];
    hit.clip.y += translation[1];
    let anchor = visible_point(&hit).unwrap();
    assert!(hit.contains(anchor));
    for axis in 0..2 {
        assert!((anchor[axis] - original[axis] - translation[axis]).abs() < 1e-12);
    }
    let mut hits = [hit];
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    let output = sample(&mut nav, &hits, &targets, input(1, RIGHT));
    assert_eq!(
        output.selection.unwrap().target.identity,
        targets[0].identity
    );
    assert_eq!(output.cursor, Some(anchor));

    hits[0].clip.x += 6.0;
    hits[0].clip.w = 4.0;
    assert_eq!(visible_point(&hits[0]), None);
    let output = sample(&mut nav, &hits, &targets, input(2, NEUTRAL));
    assert!(output.selection.is_none());
    assert!(output.cursor.is_none());
}

#[test]
fn huge_elapsed_repeat_skips_authored_wrap_cycles_without_a_timing_cap() {
    let mut hits = vec![
        region(0, 10.0, 10.0),
        region(1, 30.0, 10.0),
        region(2, 50.0, 10.0),
    ];
    for (index, hit) in hits.iter_mut().enumerate() {
        let focus = hit.focus.as_mut().unwrap();
        focus.wrap = true;
        focus.change[3] = format!("id{}", (index + 1) % 3);
    }
    let targets = targets(&hits);
    let mut nav = Navigation::new(Settings {
        repeat_delay: Duration::from_nanos(1),
        repeat_interval: Duration::from_nanos(1),
        ..navigation().settings
    })
    .unwrap();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(0, RIGHT));
    let output = sample(&mut nav, &hits, &targets, input(u64::MAX - 1, RIGHT));
    let expected = (Duration::from_millis(u64::MAX - 1).as_nanos() % 3) as u64;
    assert_eq!(output.selection.unwrap().target.identity, expected);
    assert_eq!(output.cursor, Some([15.0 + expected as f64 * 20.0, 15.0]));
}

#[test]
fn invalid_geometry_is_never_focusable_and_invalid_viewports_cancel_input() {
    let mut hits = vec![region(0, 10.0, 10.0), region(1, 30.0, 10.0)];
    let targets = targets(&hits);
    hits[0].rect.x = f64::NAN;
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(1, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        1
    );
    for invalid in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        let output = nav.update(
            Frame {
                generation: 1,
                geometry_revision: 1,
                bounds: RectOut {
                    w: invalid,
                    ..bounds()
                },
                hits: &hits,
                targets: &targets,
            },
            input(2, RIGHT),
        );
        assert_eq!(output.mode, Mode::Inactive);
        assert!(output.selection.is_none());
        assert!(output.cursor.is_none());
    }
    let mut hit = region(0, 10.0, 10.0);
    for invalid in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        hit.rect.w = invalid;
        assert!(visible_point(&hit).is_none());
    }
}

#[test]
fn nonfocusable_mapping_carriers_keep_their_authored_routes() {
    let mut hits = vec![
        region(0, 10.0, 10.0),
        region(1, 30.0, 10.0),
        region(2, 50.0, 10.0),
    ];
    let targets = targets(&hits);
    let mut carrier = region(3, 0.0, 0.0);
    carrier.rect.w = 0.0;
    let mut changes: [String; 4] = Default::default();
    changes[3] = "id2".into();
    carrier
        .focus
        .as_mut()
        .unwrap()
        .mapping
        .push(("id0".into(), changes));
    hits.push(carrier);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    sample(&mut nav, &hits, &targets, input(2, NEUTRAL));
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(3, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        2
    );
}

#[test]
fn authored_container_stop_prevents_cross_panel_geometric_fallback() {
    let mut hits = vec![region(0, 10.0, 10.0), region(1, 80.0, 10.0)];
    hits[0].key = "first/slot".into();
    hits[1].key = "second/slot".into();
    hits[0]
        .focus
        .as_mut()
        .unwrap()
        .containers
        .push(json_ui::FocusContainer {
            key: "first".into(),
            name: "first".into(),
            rect: RectOut {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 100.0,
            },
            use_last_focus: true,
            wrap: false,
            modes: [json_ui::NavigationMode::Stop; 4],
            custom: Default::default(),
        });
    let targets = targets(&hits);
    let mut nav = navigation();
    sample(&mut nav, &hits, &targets, input(0, NEUTRAL));
    sample(&mut nav, &hits, &targets, input(1, RIGHT));
    assert_eq!(
        sample(&mut nav, &hits, &targets, input(301, RIGHT))
            .selection
            .unwrap()
            .target
            .identity,
        0
    );
}
