//! The showcase's mobs on a flat surface: the void walker, the lenticular mob, the liquid metal
//! and thermal vision.

use super::flat::{Inputs, at, brightness, raster, texture};
use super::shader;
use crate::gpu_snapshot::Gpu;

/// The animation phase `seconds` into the session clock.
fn phase(seconds: f32) -> f32 {
    seconds / 4096.0
}

#[test]
fn the_void_walkers_stars_stay_in_the_world_while_its_body_moves() {
    let Some(gpu) = Gpu::for_fixture("material fx void walker") else {
        return;
    };
    let params = [0.75, 0.55, 1.0, 1.0];
    let frame = |origin: [f32; 3], view: [f32; 3]| {
        raster(
            &gpu,
            shader!("voidwalker"),
            &[],
            &params,
            &Inputs {
                origin,
                view,
                phase: phase(2.0),
                ..Inputs::default()
            },
        )
    };
    let here = frame([0.0; 3], [0.0, 0.0, 1.0]);
    let moved = frame([3.0, 1.0, -2.0], [0.0, 0.0, 1.0]);
    assert_eq!(
        here, moved,
        "the same view shows the same sky wherever the body is"
    );
    let turned = frame([0.0; 3], [0.3, 0.1, 0.95]);
    assert!(here != turned, "looking elsewhere shows other stars");
    let rim = raster(
        &gpu,
        shader!("voidwalker"),
        &[],
        &params,
        &Inputs {
            tilt: 8.0,
            phase: phase(2.0),
            ..Inputs::default()
        },
    );
    let edge = at(&rim, 0.99, 0.5);
    assert!(
        edge[2] > 200 && edge[0] > 120,
        "a violet rim glows where the body turns away: {edge:?}"
    );
}

#[test]
fn the_lenticular_mob_shows_another_face_from_each_side() {
    let Some(gpu) = Gpu::for_fixture("material fx lenticular") else {
        return;
    };
    let faces = || {
        texture(&gpu, 192, |x, _| match x / 64 {
            0 => [255, 0, 0, 255],
            1 => [0, 255, 0, 255],
            _ => [0, 0, 255, 255],
        })
    };
    let seen = |view: [f32; 3]| {
        let frame = raster(
            &gpu,
            shader!("lenticular"),
            &[faces()],
            &[0.45],
            &Inputs {
                view,
                ..Inputs::default()
            },
        );
        let p = at(&frame, 0.5, 0.5);
        (0..3).max_by_key(|&channel| p[channel]).unwrap()
    };
    assert_eq!(seen([0.0, 0.0, 1.0]), 1, "head on, the middle face");
    assert_eq!(seen([-0.6, 0.0, 0.8]), 0);
    assert_eq!(seen([0.6, 0.0, 0.8]), 2);
}

#[test]
fn the_liquid_metal_mirrors_its_sky_and_ripples_when_struck() {
    let Some(gpu) = Gpu::for_fixture("material fx metal") else {
        return;
    };
    // A skybox blue above the horizon and brown below.
    let sky = || {
        texture(&gpu, 64, |x, y| match (y < 32, x % 2 == 0) {
            (true, true) => [70, 120, 230, 255],
            (true, false) => [40, 160, 255, 255],
            (false, _) => [120, 80, 40, 255],
        })
    };
    let struck = |seconds: f32| {
        raster(
            &gpu,
            shader!("metal"),
            &[sky()],
            &[2.0, 2.0, 0.0, 10.0],
            &Inputs {
                normal: [0.0, 0.5, 0.86],
                phase: phase(seconds),
                ..Inputs::default()
            },
        )
    };
    let calm = struck(60.0);
    let centre = at(&calm, 0.5, 0.5);
    assert!(
        centre[2] > centre[0],
        "a surface tilted up mirrors the sky: {centre:?}"
    );
    assert!(struck(10.15) != calm, "a hit ripples the reflections");
}

#[test]
fn thermal_vision_runs_hot_where_the_body_faces_the_viewer() {
    let Some(gpu) = Gpu::for_fixture("material fx thermal") else {
        return;
    };
    let frame = raster(
        &gpu,
        shader!("thermal"),
        &[],
        &[0.85, 0.0],
        &Inputs {
            tilt: 6.0,
            phase: phase(1.0),
            ..Inputs::default()
        },
    );
    let (core, edge) = (at(&frame, 0.5, 0.3), at(&frame, 0.99, 0.3));
    assert!(
        brightness(core) > brightness(edge) + 100 && core[1] > edge[1],
        "{core:?} {edge:?}"
    );
}
