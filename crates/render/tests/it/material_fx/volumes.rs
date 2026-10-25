//! The volumetric showcase pieces seen through real camera rays: the ship in a bottle, the snow
//! globe, the lava lamp, the black hole, the hollow blocks, the carved rune and the portal.

use super::flat::{brightness, texture};
use super::scene::{Camera, MISS, Scene, view};
use super::shader;
use crate::gpu_snapshot::Gpu;

/// The animation phase `seconds` into the session clock.
fn phase(seconds: f32) -> f32 {
    seconds / 4096.0
}

/// The pixel at `(u, v)` of a frame, both in `[0, 1)`, v down.
fn pixel(frame: &[u8], u: f32, v: f32) -> [u8; 4] {
    super::flat::at(frame, u, v)
}

/// Every pixel of a frame.
fn pixels(frame: &[u8]) -> impl Iterator<Item = [u8; 4]> + '_ {
    frame.as_chunks::<4>().0.iter().copied()
}

/// Cameras on a block from the front, the side, above and a diagonal, all aimed at its centre.
fn around(target: [f32; 3]) -> [Camera; 4] {
    let [x, y, z] = target;
    [
        [x, y, z + 3.5],
        [x + 3.5, y, z],
        [x + 0.3, y + 3.5, z + 0.2],
        [x + 2.2, y + 2.0, z + 2.6],
    ]
    .map(|eye| Camera {
        eye,
        target,
        fov: 40.0,
    })
}

/// A diorama texture holding only the 2×2×2 red voxel cube at voxels 7..9, with every voxel's
/// Chebyshev distance to it in alpha, as the `material-fx` tool packs its grid.
fn one_cube(gpu: &Gpu) -> wgpu::TextureView {
    texture(gpu, 64, |tx, ty| {
        let (tile_x, tile_y) = (tx / 16, ty / 16);
        let y = (tile_y * 4 + tile_x) as i32;
        let (x, z) = ((tx % 16) as i32, (ty % 16) as i32);
        let gap = |v: i32| {
            if v < 7 {
                7 - v
            } else if v > 8 {
                v - 8
            } else {
                0
            }
        };
        let distance = gap(x).max(gap(y)).max(gap(z)).min(15);
        let solid = distance == 0;
        let alpha = (distance * 255 / 15) as u8;
        if solid {
            [230, 40, 30, 0]
        } else {
            [0, 0, 0, alpha]
        }
    })
}

#[test]
fn the_bottle_shows_its_voxel_at_its_place_from_every_angle_and_glass_elsewhere() {
    let Some(gpu) = Gpu::for_fixture("material fx bottle") else {
        return;
    };
    for camera in around([0.5; 3]) {
        let frame = view(
            &gpu,
            shader!("bottle"),
            &[one_cube(&gpu)],
            &[1.0e6, 0.12],
            &camera,
            &Scene::default(),
        );
        let centre = pixel(&frame, 0.5, 0.5);
        assert!(
            centre[3] == 255 && centre[0] > 100 && u16::from(centre[0]) > u16::from(centre[1]) * 3,
            "the ray through the block's centre meets the red voxel from {:?}: {centre:?}",
            camera.eye
        );
        // Away from the voxel, rays see faint glass and the world behind it.
        let glass = pixels(&frame).filter(|p| *p != MISS && p[3] < 128).count();
        assert!(
            glass > 2000,
            "most of the bottle is see-through from {:?}: {glass}",
            camera.eye
        );
    }
}

#[test]
fn the_snow_globe_holds_a_tree_at_its_depth_and_snow_moves() {
    let Some(gpu) = Gpu::for_fixture("material fx snow globe") else {
        return;
    };
    for camera in around([0.5, 0.52, 0.5]).into_iter().take(2) {
        let frame = view(
            &gpu,
            shader!("snowglobe"),
            &[],
            &[3.0, 0.1],
            &camera,
            &Scene::default(),
        );
        let green = (0..16)
            .flat_map(|i| (0..16).map(move |j| (i, j)))
            .map(|(i, j)| pixel(&frame, 0.46 + i as f32 * 0.005, 0.46 + j as f32 * 0.005))
            .filter(|p| p[1] > p[0] && p[1] > p[2] && p[3] == 255)
            .count();
        assert!(
            green > 128,
            "the pine stands in the middle from {:?}: {green}",
            camera.eye
        );
    }
    let camera = &around([0.5; 3])[0];
    let at = |seconds: f32| {
        view(
            &gpu,
            shader!("snowglobe"),
            &[],
            &[3.0, 0.1],
            camera,
            &Scene {
                phase: phase(seconds),
                ..Scene::default()
            },
        )
    };
    let (now, later) = (at(10.0), at(10.7));
    let flakes = |frame: &[u8]| {
        pixels(frame)
            .filter(|p| p[0] > 240 && p[1] > 240 && p[2] > 240 && p[3] == 255)
            .count()
    };
    assert!(
        flakes(&now) > 20,
        "snow falls in the globe: {}",
        flakes(&now)
    );
    assert!(now != later, "the snow moves");
}

#[test]
fn the_lava_lamp_glows_with_blobs_that_rise_and_sink() {
    let Some(gpu) = Gpu::for_fixture("material fx lava lamp") else {
        return;
    };
    let camera = &around([0.5; 3])[0];
    let at = |seconds: f32| {
        view(
            &gpu,
            shader!("lavalamp"),
            &[],
            &[1.0, 0.55, 0.15, 1.0],
            camera,
            &Scene {
                phase: phase(seconds),
                ..Scene::default()
            },
        )
    };
    let (now, later) = (at(3.0), at(5.0));
    let blob = |frame: &[u8]| {
        pixels(frame)
            .filter(|p| p[0] > 200 && p[1] > 80 && p[2] < 90)
            .count()
    };
    assert!(
        blob(&now) > 200,
        "orange blobs glow in the violet: {}",
        blob(&now)
    );
    assert!(blob(&now) != blob(&later), "the blobs move");
}

#[test]
fn the_black_hole_swallows_its_centre_bends_a_bright_ring_and_fades_out_at_its_reach() {
    let Some(gpu) = Gpu::for_fixture("material fx black hole") else {
        return;
    };
    let scene = Scene {
        lo: [0.0; 3],
        hi: [3.0; 3],
        ..Scene::default()
    };
    let camera = Camera {
        eye: [1.5, 1.7, 9.0],
        target: [1.5, 1.5, 1.5],
        fov: 30.0,
    };
    let frame = view(
        &gpu,
        shader!("blackhole"),
        &[],
        &[1.5, 1.5, 1.5, 0.16, 1.5],
        &camera,
        &scene,
    );
    let centre = pixel(&frame, 0.5, 0.5);
    assert!(
        centre[3] == 255 && brightness(centre) < 40,
        "the shadow is black: {centre:?}"
    );
    let brightest = pixels(&frame)
        .filter(|p| *p != MISS)
        .map(brightness)
        .max()
        .unwrap();
    assert!(
        brightest > 600,
        "the disk and the photon ring blaze: {brightest}"
    );
    // The box's corners lie outside the lensing radius: the world behind shows through.
    let corner = pixels(&frame).filter(|p| *p != MISS && p[3] == 0).count();
    assert!(
        corner > 1000,
        "outside its reach the cluster is clear: {corner}"
    );
}

#[test]
fn hollow_blocks_open_onto_an_aquarium_and_a_torchlit_mineshaft() {
    let Some(gpu) = Gpu::for_fixture("material fx hollow") else {
        return;
    };
    let camera = &around([0.5; 3])[0];
    let aquarium = view(
        &gpu,
        shader!("hollow"),
        &[],
        &[0.0, 3.0],
        camera,
        &Scene {
            phase: phase(5.0),
            ..Scene::default()
        },
    );
    let middle = pixel(&aquarium, 0.5, 0.42);
    assert!(
        middle[2] > middle[0],
        "the aquarium is water-blue: {middle:?}"
    );
    let shaft = view(
        &gpu,
        shader!("hollow"),
        &[],
        &[1.0, 10.0],
        camera,
        &Scene {
            phase: phase(5.0),
            ..Scene::default()
        },
    );
    let flame = pixels(&shaft)
        .filter(|p| p[0] > 200 && p[1] > 150 && p[2] < 140)
        .count();
    assert!(flame > 0, "the torch burns at the end of the shaft");
    // The view into the shaft changes with the angle: a deep room, not a painted face.
    let aside = view(
        &gpu,
        shader!("hollow"),
        &[],
        &[1.0, 10.0],
        &around([0.5; 3])[3],
        &Scene {
            phase: phase(5.0),
            ..Scene::default()
        },
    );
    assert!(shaft != aside);
}

#[test]
fn the_rune_cuts_glow_and_their_walls_shift_with_the_view() {
    let Some(gpu) = Gpu::for_fixture("material fx rune") else {
        return;
    };
    // One vertical groove down the middle of the face.
    let rune = || {
        texture(&gpu, 16, |x, _| {
            if x == 7 || x == 8 {
                [0, 0, 0, 0]
            } else {
                [255; 4]
            }
        })
    };
    let looking = |eye: [f32; 3]| {
        view(
            &gpu,
            shader!("rune"),
            &[rune()],
            &[0.15, 0.3, 0.8, 1.0],
            &Camera {
                eye,
                target: [0.5, 0.5, 1.0],
                fov: 30.0,
            },
            &Scene::default(),
        )
    };
    let ahead = looking([0.5, 0.5, 4.0]);
    let glow = |frame: &[u8]| {
        pixels(frame)
            .filter(|p| p[2] > 180 && u16::from(p[2]) > u16::from(p[0]) + 60)
            .count()
    };
    assert!(glow(&ahead) > 100, "the groove glows: {}", glow(&ahead));
    let aside = looking([2.5, 0.5, 3.0]);
    assert!(
        glow(&aside) < glow(&ahead),
        "seen at an angle, the groove's wall hides part of its floor"
    );
}

#[test]
fn the_portal_tunnel_turns_and_burns_brighter_as_the_player_nears() {
    let Some(gpu) = Gpu::for_fixture("material fx portal") else {
        return;
    };
    let stars = || {
        texture(&gpu, 16, |x, y| {
            if (x * 7 + y * 3) % 11 == 0 {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            }
        })
    };
    let scene = Scene {
        lo: [0.0; 3],
        hi: [2.0, 3.0, 1.0],
        phase: phase(7.0),
        ..Scene::default()
    };
    let camera = Camera {
        eye: [1.0, 1.5, -4.0],
        target: [1.0, 1.5, 0.5],
        fov: 40.0,
    };
    let at = |distance: f32| {
        view(
            &gpu,
            shader!("vortex"),
            &[stars()],
            &[1.0, 1.5, 0.5, 0.0, distance, 0.45, 0.15, 0.85],
            &camera,
            &scene,
        )
    };
    let total = |frame: &[u8]| {
        pixels(frame)
            .filter(|p| *p != MISS)
            .map(brightness)
            .map(u64::from)
            .sum::<u64>()
    };
    let (far, near) = (at(20.0), at(1.0));
    assert!(
        total(&near) > total(&far) * 11 / 10,
        "it brightens as the player nears: {} vs {}",
        total(&near),
        total(&far)
    );
    let violet = pixels(&far)
        .filter(|p| u16::from(p[2]) > u16::from(p[1]) + 40 && p[0] > p[1])
        .count();
    assert!(violet > 2000, "the tunnel spirals in violet: {violet}");
}

/// The perf budget: every copy of a raymarched block traces only its own block, within the
/// sandbox's fixed per-pixel worst case (`every_showcase_shader_passes_the_sandbox`), so a wall
/// of copies costs what one block costs per covered pixel. Sixteen bottles side by side each
/// show their own voxel.
#[test]
fn a_wall_of_bottles_traces_each_copy_on_its_own() {
    let Some(gpu) = Gpu::for_fixture("material fx bottle wall") else {
        return;
    };
    let scene = Scene {
        lo: [0.0; 3],
        hi: [4.0, 4.0, 1.0],
        ..Scene::default()
    };
    let camera = Camera {
        eye: [2.0, 2.0, 60.0],
        target: [2.0, 2.0, 0.5],
        fov: 4.2,
    };
    let frame = view(
        &gpu,
        shader!("bottle"),
        &[one_cube(&gpu)],
        &[1.0e6, 0.12],
        &camera,
        &scene,
    );
    for row in 0..4 {
        for column in 0..4 {
            let p = pixel(
                &frame,
                (column as f32 + 0.5) / 4.0 * 0.9 + 0.05,
                (row as f32 + 0.5) / 4.0 * 0.9 + 0.05,
            );
            assert!(
                p[3] == 255 && p[0] > 100,
                "copy ({column}, {row}) shows its voxel: {p:?}"
            );
        }
    }
}
