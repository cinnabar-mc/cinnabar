//! The showcase's surface blocks on a flat face: the arena floor's shockwave and heartbeat, the
//! breathing ores, the corruption, the living painting and the screen's radar.

use super::flat::{Inputs, at, brightness, raster, texture};
use super::shader;
use crate::gpu_snapshot::Gpu;

/// The animation phase `seconds` into the session clock.
fn phase(seconds: f32) -> f32 {
    seconds / 4096.0
}

/// Every pixel of a frame.
fn pixels(frame: &[u8]) -> impl Iterator<Item = [u8; 4]> + '_ {
    frame.as_chunks::<4>().0.iter().copied()
}

/// An 8×8-block floor seen from above, x across and z down the frame.
fn floor(seconds: f32) -> Inputs {
    Inputs {
        phase: phase(seconds),
        normal: [0.0, 1.0, 0.0],
        view: [0.0, 1.0, 0.0],
        across: [8.0, 0.0, 0.0],
        up: [0.0, 0.0, 8.0],
        ..Inputs::default()
    }
}

#[test]
fn the_shockwave_ring_sits_where_it_has_travelled_and_the_heartbeat_pulses() {
    let Some(gpu) = Gpu::for_fixture("material fx arena") else {
        return;
    };
    // Impact at (4, 4) at t = 100 s; a quarter second later the ring has run 2.25 blocks.
    let params = [4.0, 4.0, 100.0, 0.0, 1.0, 4.0, 4.0];
    let frame = raster(&gpu, shader!("arena"), &[], &params, &floor(100.25));
    // The frame's v runs along z.
    let ring = at(&frame, (4.0 + 2.25) / 8.0, 1.0 - 4.0 / 8.0);
    let inside = at(&frame, (4.0 + 1.0) / 8.0, 1.0 - 4.0 / 8.0);
    assert!(
        ring[2] > 200 && brightness(ring) > brightness(inside) + 200,
        "{ring:?} {inside:?}"
    );
    let settled = raster(&gpu, shader!("arena"), &[], &params, &floor(110.0));
    let brightest = pixels(&settled).map(brightness).max().unwrap();
    assert!(
        brightest < 400,
        "long after, only the plates remain: {brightest}"
    );
    // The heartbeat lights the floor red on its beat.
    let heart = [4.0, 4.0, 0.0, 1.0, 0.3, 4.0, 4.0];
    let beats = (0..16)
        .map(|step| {
            let frame = raster(
                &gpu,
                shader!("arena"),
                &[],
                &heart,
                &floor(200.0 + step as f32 * 0.04),
            );
            pixels(&frame).filter(|p| p[0] > 180 && p[1] < 80).count()
        })
        .collect::<Vec<_>>();
    assert!(
        beats.iter().max() > beats.iter().min().map(|m| m + 1000).as_ref(),
        "{beats:?}"
    );
}

#[test]
fn diamond_glints_follow_the_view_and_redstone_pulses_travel() {
    let Some(gpu) = Gpu::for_fixture("material fx ore") else {
        return;
    };
    let lit = |frame: &[u8]| pixels(frame).filter(|p| brightness(*p) > 600).count();
    let looking = |view: [f32; 3]| {
        raster(
            &gpu,
            shader!("ore"),
            &[],
            &[0.0, 1.0],
            &Inputs {
                view,
                ..Inputs::default()
            },
        )
    };
    let (ahead, aside) = (looking([0.0, 0.0, 1.0]), looking([0.5, 0.4, 0.77]));
    assert!(lit(&ahead) + lit(&aside) > 0, "diamonds glint");
    assert!(ahead != aside, "the glints follow the view");
    let pulse = |seconds: f32| {
        raster(
            &gpu,
            shader!("ore"),
            &[],
            &[1.0, 1.0],
            &Inputs {
                phase: phase(seconds),
                ..Inputs::default()
            },
        )
    };
    let (now, later) = (pulse(1.0), pulse(2.5));
    let red = |frame: &[u8]| pixels(frame).filter(|p| p[0] > 200 && p[1] < 60).count();
    assert!(red(&now) > 100, "redstone veins glow: {}", red(&now));
    assert!(now != later, "pulses travel along the veins");
}

#[test]
fn corruption_spreads_from_its_source_after_it_starts() {
    let Some(gpu) = Gpu::for_fixture("material fx corruption") else {
        return;
    };
    let purple = |frame: &[u8]| {
        pixels(frame)
            .filter(|p| {
                u16::from(p[2]) > u16::from(p[1]) + 40 && u16::from(p[0]) > u16::from(p[1]) + 20
            })
            .count()
    };
    let at_time = |params: &[f32], seconds: f32| {
        raster(
            &gpu,
            shader!("corruption"),
            &[],
            params,
            &Inputs {
                phase: phase(seconds),
                ..Inputs::default()
            },
        )
    };
    let clean = at_time(&[0.5, 0.5, 0.5, -1.0, 1.2], 50.0);
    assert_eq!(purple(&clean), 0, "without a start the stone stays clean");
    let started = [0.5, 0.5, 0.5, 40.0, 1.2];
    let (early, late) = (
        purple(&at_time(&started, 40.2)),
        purple(&at_time(&started, 46.0)),
    );
    assert!(
        late > early + 5000,
        "the infection creeps outward: {early} then {late}"
    );
}

#[test]
fn the_painted_owl_watches_the_viewer() {
    let Some(gpu) = Gpu::for_fixture("material fx painting") else {
        return;
    };
    // Plain dark canvas with the two 3×3 eye whites the art tags.
    let art = || {
        texture(&gpu, 32, |x, y| {
            let eye = |cx: u32| x.abs_diff(cx) <= 1 && y.abs_diff(10) <= 1;
            if eye(13) || eye(19) {
                [255, 216, 77, 153]
            } else {
                [40, 40, 60, 255]
            }
        })
    };
    let canvas = |view: [f32; 3]| Inputs {
        view,
        origin: [0.0, 0.0, 1.0],
        across: [2.0, 0.0, 0.0],
        up: [0.0, 2.0, 0.0],
        phase: phase(1.0),
        ..Inputs::default()
    };
    let pupil = |view: [f32; 3]| {
        let frame = raster(
            &gpu,
            shader!("canvas"),
            &[art()],
            &[1.0, 1.0, 0.5, 0.0, 0.0, 1.0],
            &canvas(view),
        );
        // The darkest canvas pixel around the left eye.
        (12..=14)
            .flat_map(|x| (20..=22).map(move |y| (x, y)))
            .min_by_key(|&(x, y)| {
                brightness(at(&frame, (x as f32 + 0.5) / 32.0, (y as f32 + 0.5) / 32.0))
            })
            .unwrap()
    };
    let (left, right) = (pupil([-0.7, 0.0, 0.7]), pupil([0.7, 0.0, 0.7]));
    assert!(
        right.0 > left.0,
        "the pupils move towards the viewer: {left:?} {right:?}"
    );
}

#[test]
fn the_screen_shows_each_player_as_a_blip() {
    let Some(gpu) = Gpu::for_fixture("material fx screen") else {
        return;
    };
    let face = Inputs {
        normal: [0.0, 0.0, -1.0],
        view: [0.0, 0.0, -1.0],
        across: [4.0, 0.0, 0.0],
        up: [0.0, 3.0, 0.0],
        origin: [0.0, 0.0, 0.0],
        phase: phase(3.0),
        ..Inputs::default()
    };
    let blips = |count: f32| {
        let mut params = vec![
            2.0, 1.5, 0.5, 0.0, 0.5, 2.0, 0.5, 6.0, 4.0, 0.0, 0.0, 0.0, 0.0, count,
        ];
        params.truncate(14);
        let frame = raster(&gpu, shader!("screen"), &[], &params, &face);
        pixels(&frame)
            .filter(|p| p[0] > 200 && p[1] > 200 && p[2] < 160)
            .count()
    };
    assert_eq!(blips(0.0), 0, "an empty radar shows no blips");
    assert!(blips(2.0) > 0, "players show as blips");
}
