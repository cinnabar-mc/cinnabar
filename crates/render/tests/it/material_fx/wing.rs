//! The effects wing's materials through the host's guest library: displacement in whole
//! texels where each effect moves geometry, scene reads where each bends or softens the world
//! behind it, the boss's outline colour, and the storm's and fire's compute steps.

use super::effects::run_compute;
use super::flat::{Effects, Inputs, at, raster_effects, raster_with_effects};
use super::shader;
use crate::gpu_snapshot::{Gpu, SNAPSHOT_SIDE};

use mod_render::world_material::Features;

/// How many blocks of displacement one step of a channel encodes: offsets in `[-2, 2]` map to
/// `[0, 1]`.
const SPAN: f32 = 4.0;

/// Rasters `source`'s clamped displacement of the surface `inputs` describe, within `limit`
/// blocks, as colour: each axis `0.5 + offset / SPAN`.
fn displacement(gpu: &Gpu, source: &str, params: &[f32], limit: f32, inputs: &Inputs) -> Vec<u8> {
    let effects = Effects {
        features: Features {
            displace: true,
            ..Default::default()
        },
        rows: [[limit, 0.0, 0.0, 0.0], [0.0; 4]],
        ..Effects::default()
    };
    let v = |v: [f32; 3]| format!("vec3<f32>({:?}, {:?}, {:?})", v[0], v[1], v[2]);
    let (origin, across, up, normal) = (
        v(inputs.origin),
        v(inputs.across),
        v(inputs.up),
        v(inputs.normal),
    );
    let phase = inputs.phase;
    raster_with_effects(gpu, source, &[], params, &effects, |library| {
        format!(
            "{library}
struct FxVertex {{ @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }}
@vertex fn fx_vertex(@builtin(vertex_index) index: u32) -> FxVertex {{
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return FxVertex(vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0), vec2<f32>(corner.x, 1.0 - corner.y));
}}
@fragment fn fx_fragment(vertex: FxVertex) -> @location(0) vec4<f32> {{
    let uv = clamp(vertex.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let world = {origin} + uv.x * {across} + uv.y * {up};
    let offset = world_material_displace(WorldVertexInput(world, {normal}, uv, {phase:?}));
    return vec4<f32>(clamp(vec3<f32>(0.5) + offset / {SPAN:?}, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}}
"
        )
    })
}

/// The displacement at `(u, v)` of a [`displacement`] frame, in blocks.
fn offset_at(frame: &[u8], u: f32, v: f32) -> [f32; 3] {
    let pixel = at(frame, u, v);
    [0, 1, 2].map(|axis| (f32::from(pixel[axis]) / 255.0 - 0.5) * SPAN)
}

/// Whether every axis of `offset` lies on the 16-texel grid, within the encoding's step.
fn whole_texels(offset: [f32; 3]) -> bool {
    let step = SPAN / 255.0 * 16.0;
    offset
        .iter()
        .all(|axis| (axis * 16.0 - (axis * 16.0).round()).abs() <= step)
}

/// The largest axis of `offset`.
fn size(offset: [f32; 3]) -> f32 {
    offset
        .iter()
        .fold(0.0_f32, |most, axis| most.max(axis.abs()))
}

/// The session phase at `seconds`.
fn phase(seconds: f32) -> f32 {
    seconds / 4096.0
}

#[test]
fn the_grass_tops_sway_in_whole_texels_while_their_bottoms_stay() {
    let Some(gpu) = Gpu::for_fixture("waving grass") else {
        return;
    };
    let grass = shader!("wavegrass");
    // A side face from the ground (y 0) to the field's top (y 1), seen at several moments.
    let side = |seconds: f32| Inputs {
        origin: [0.0, 0.0, 0.0],
        across: [4.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        phase: phase(seconds),
        ..Inputs::default()
    };
    let mut swayed = 0.0_f32;
    for seconds in [10.0, 11.3, 12.7, 14.1] {
        let frame = displacement(&gpu, grass, &[1.0, 1.2, 1.0], 0.25, &side(seconds));
        let (bottom, top) = (offset_at(&frame, 0.3, 0.02), offset_at(&frame, 0.3, 0.98));
        assert!(size(bottom) < 0.02, "the bottom stays: {bottom:?}");
        assert!(whole_texels(top), "{top:?}");
        assert!(
            size(top) <= 0.25 + 0.02,
            "within the declared reach: {top:?}"
        );
        assert!(top[1].abs() < 0.02, "the grass never stretches up: {top:?}");
        swayed = swayed.max(size(top));
    }
    assert!(swayed >= 1.0 / 16.0, "the wind moves the tops");
    // Without wind nothing moves.
    let calm = displacement(&gpu, grass, &[0.0, 1.2, 1.0], 0.25, &side(10.0));
    assert!(size(offset_at(&calm, 0.3, 0.98)) < 0.02);
}

#[test]
fn the_flag_folds_more_away_from_its_pole_in_whole_texels() {
    let Some(gpu) = Gpu::for_fixture("flag") else {
        return;
    };
    let flag = shader!("flag");
    // The cloth spans x from -1.5 (the pole, facing south) to 1.5 about its centre.
    let cloth = |seconds: f32| Inputs {
        origin: [-1.5, -1.0, 0.0],
        across: [3.0, 0.0, 0.0],
        up: [0.0, 2.0, 0.0],
        phase: phase(seconds),
        ..Inputs::default()
    };
    let (mut near, mut far) = (0.0_f32, 0.0_f32);
    for seconds in [3.0, 3.4, 3.9, 4.5, 5.2] {
        let frame = displacement(&gpu, flag, &[0.0, 0.0, 0.0, 0.0, 1.0], 0.5, &cloth(seconds));
        let (pole, end) = (offset_at(&frame, 0.01, 0.5), offset_at(&frame, 0.99, 0.5));
        assert!(whole_texels(pole) && whole_texels(end), "{pole:?} {end:?}");
        // Out of the plane only: along z for a flag facing south.
        assert!(end[0].abs() < 0.02 && end[1].abs() < 0.02, "{end:?}");
        near = near.max(size(pole));
        far = far.max(size(end));
    }
    assert!(near < 0.07, "the pole holds the cloth: {near}");
    assert!(far >= 0.25, "the free end flaps: {far}");
}

#[test]
fn the_jelly_shakes_after_a_hit_and_settles_to_breathing() {
    let Some(gpu) = Gpu::for_fixture("jelly") else {
        return;
    };
    let jelly = shader!("jelly");
    let skin = |seconds: f32| Inputs {
        origin: [-0.5, 0.0, 0.5],
        across: [1.0, 0.0, 0.0],
        up: [0.0, 2.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        phase: phase(seconds),
        ..Inputs::default()
    };
    // Hit at second 100; the most it moves in each window.
    let hit = [0.0, 0.0, 0.0, 100.0];
    let most = |from: f32| {
        (0..6)
            .map(|step| {
                let frame = displacement(&gpu, jelly, &hit, 0.5, &skin(from + step as f32 * 0.017));
                let offset = offset_at(&frame, 0.5, 0.7);
                assert!(whole_texels(offset), "{offset:?}");
                size(offset)
            })
            .fold(0.0_f32, f32::max)
    };
    let shaking = most(100.05);
    let settled = most(104.0);
    assert!(shaking >= 0.12, "{shaking}");
    assert!(settled <= 1.0 / 16.0 + 0.02, "only breathing: {settled}");
}

#[test]
fn the_shattering_mob_flies_apart_in_whole_texels_and_comes_back_whole() {
    let Some(gpu) = Gpu::for_fixture("shatter") else {
        return;
    };
    let shatter = shader!("shatter");
    let skin = |seconds: f32| Inputs {
        origin: [-0.5, 0.0, 0.5],
        across: [1.0, 0.0, 0.0],
        up: [0.0, 2.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        phase: phase(seconds),
        base: [0.8, 0.3, 0.2, 1.0],
        ..Inputs::default()
    };
    // A burst started at second 50 of a four-second cycle: out at 51.5, back by 54.
    let burst = [4.0, 50.0];
    let apart = displacement(&gpu, shatter, &burst, 2.0, &skin(51.5));
    let offset = offset_at(&apart, 0.4, 0.4);
    assert!(whole_texels(offset) && size(offset) >= 0.5, "{offset:?}");
    let whole = displacement(&gpu, shatter, &burst, 2.0, &skin(53.95));
    assert!(size(offset_at(&whole, 0.4, 0.4)) < 0.02);
    // Mid-burst most texels have dropped out; once back, none.
    let drawn = |seconds: f32| {
        let frame = raster_effects(
            &gpu,
            shatter,
            &[],
            &burst,
            &skin(seconds),
            &Effects::default(),
        );
        frame.chunks(4).filter(|pixel| pixel[3] > 128).count()
    };
    let total = (SNAPSHOT_SIDE * SNAPSHOT_SIDE) as usize;
    assert!(drawn(51.5) < total / 2, "{}", drawn(51.5));
    assert_eq!(drawn(53.95), total);
}

/// A scene of red on the left half and blue on the right, two blocks away.
fn halves_scene(gpu: &Gpu) -> (wgpu::TextureView, wgpu::TextureView) {
    let colour = super::effects::halves(
        gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        [255, 0, 0, 255, 0, 0, 0, 0],
        [0, 0, 255, 255, 0, 0, 0, 0],
    );
    let two = 0x4000_u16.to_le_bytes();
    let depth = [two[0], two[1], 0, 0, 0, 0, 0, 0];
    let distance = super::effects::halves(gpu, wgpu::TextureFormat::R16Float, depth, depth);
    (colour, distance)
}

/// The scene effect over `scene`.
fn reading<'a>(scene: &'a (wgpu::TextureView, wgpu::TextureView)) -> Effects<'a> {
    Effects {
        features: Features {
            scene: true,
            ..Default::default()
        },
        scene: Some((&scene.0, &scene.1)),
        ..Effects::default()
    }
}

/// How many pixels of `frame` are mostly red and mostly blue.
fn reds_and_blues(frame: &[u8]) -> (usize, usize) {
    let red = frame.chunks(4).filter(|p| p[0] > 150 && p[2] < 100).count();
    let blue = frame.chunks(4).filter(|p| p[2] > 150 && p[0] < 100).count();
    (red, blue)
}

#[test]
fn the_lens_shows_the_world_behind_it_bent_pane_by_pane() {
    let Some(gpu) = Gpu::for_fixture("lens") else {
        return;
    };
    let lens = shader!("lens");
    let scene = halves_scene(&gpu);
    // One block face filling the frame, seen head on.
    let face = Inputs {
        origin: [0.0, 0.0, 1.0],
        across: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        ..Inputs::default()
    };
    let pixels = 2.0 * SNAPSHOT_SIDE as f32;
    let straight = raster_effects(
        &gpu,
        lens,
        &[],
        &[0.0, pixels, 1.0],
        &face,
        &reading(&scene),
    );
    let bent = raster_effects(
        &gpu,
        lens,
        &[],
        &[1.0, pixels, 1.0],
        &face,
        &reading(&scene),
    );
    // Unbent, the panes left of centre show red and right of it blue.
    let (pane_left, pane_right) = (at(&straight, 0.3, 0.5), at(&straight, 0.7, 0.5));
    assert!(pane_left[0] > 150 && pane_left[2] < 100, "{pane_left:?}");
    assert!(pane_right[2] > 150 && pane_right[0] < 100, "{pane_right:?}");
    // Bent, some panes show the other half: the split no longer runs down the middle.
    let crossed = |frame: &[u8]| {
        (1..15)
            .flat_map(|x| (1..15).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let pixel = at(frame, (x as f32 + 0.5) / 16.0, (y as f32 + 0.5) / 16.0);
                (x < 8) != (pixel[0] > 150)
            })
            .count()
    };
    assert_eq!(crossed(&straight), 0);
    assert!(crossed(&bent) >= 6, "{}", crossed(&bent));
    // The lead frame stays dark, and without the scene the panes show their own tint.
    assert!(at(&bent, 0.02, 0.5)[..3].iter().all(|&c| c < 80));
    let plain = raster_effects(
        &gpu,
        lens,
        &[],
        &[1.0, pixels, 0.0],
        &face,
        &Effects::default(),
    );
    assert_eq!(reds_and_blues(&plain), (0, 0));
}

#[test]
fn the_heat_haze_shimmers_the_world_low_down_and_vanishes_when_cold() {
    let Some(gpu) = Gpu::for_fixture("heat haze") else {
        return;
    };
    let haze = shader!("haze");
    let scene = halves_scene(&gpu);
    let face = |seconds: f32| Inputs {
        origin: [0.0, 0.0, 1.0],
        across: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        phase: phase(seconds),
        ..Inputs::default()
    };
    // Near the middle column the bottom rows pick up both halves as the waves shift them.
    let split = |frame: &[u8], v: f32| {
        let row: Vec<bool> = (0..32)
            .map(|x| at(frame, 0.5 - 16.0 / 512.0 + x as f32 / 512.0, v)[0] > 150)
            .collect();
        row.windows(2).filter(|pair| pair[0] != pair[1]).count()
    };
    let hot = raster_effects(&gpu, haze, &[], &[1.0], &face(7.0), &reading(&scene));
    let later = raster_effects(&gpu, haze, &[], &[1.0], &face(7.2), &reading(&scene));
    assert_ne!(hot, later, "the haze moves");
    assert!(at(&hot, 0.2, 0.95)[3] > 250, "it covers the bottom");
    assert!(split(&hot, 0.9) + split(&later, 0.9) >= 1);
    // Cold, it covers nothing.
    let cold = raster_effects(&gpu, haze, &[], &[0.0], &face(7.0), &reading(&scene));
    assert!(cold.chunks(4).all(|pixel| pixel[3] == 0));
}

#[test]
fn the_lensing_black_hole_bends_the_real_scene_around_its_shadow() {
    let Some(gpu) = Gpu::for_fixture("black hole lens") else {
        return;
    };
    let hole = shader!("blackhole_lens");
    let scene = halves_scene(&gpu);
    // A face one block in front of the hole, seen head on from +z.
    let face = Inputs {
        origin: [-1.5, -1.5, 1.0],
        across: [3.0, 0.0, 0.0],
        up: [0.0, 3.0, 0.0],
        ..Inputs::default()
    };
    let params = [0.0, 0.0, 0.0, 0.16, 1.5, 700.0];
    let frame = raster_effects(&gpu, hole, &[], &params, &face, &reading(&scene));
    // The shadow at the centre is black.
    assert!(at(&frame, 0.5, 0.5)[..3].iter().all(|&c| c < 20));
    // Just right of the shadow the bent rays reach back across to the red half; far out the
    // scene shows straight, blue on the right.
    let (red, _) = reds_and_blues(&frame);
    let near_right = (0..20)
        .filter(|step| at(&frame, 0.55 + *step as f32 * 0.005, 0.5)[0] > 150)
        .count();
    assert!(red > 0 && near_right > 0, "{red} {near_right}");
    let out = at(&frame, 0.98, 0.98);
    assert!(
        out[3] < 20,
        "outside the lensing radius it is transparent: {out:?}"
    );
}

#[test]
fn the_force_field_glows_where_it_meets_what_is_behind_it() {
    let Some(gpu) = Gpu::for_fixture("force field") else {
        return;
    };
    let field = shader!("forcefield");
    // The opaque scene a tenth of a block behind on the left, three blocks on the right.
    let colour = super::effects::halves(&gpu, wgpu::TextureFormat::Rgba8Unorm, [0; 8], [0; 8]);
    // Half-float bits of 1.1 and 4.0.
    let (close, far) = (0x3c66_u16.to_le_bytes(), 0x4400_u16.to_le_bytes());
    let distance = super::effects::halves(
        &gpu,
        wgpu::TextureFormat::R16Float,
        [close[0], close[1], 0, 0, 0, 0, 0, 0],
        [far[0], far[1], 0, 0, 0, 0, 0, 0],
    );
    let scene = (colour, distance);
    let face = Inputs {
        origin: [0.0, 0.0, 0.0],
        across: [3.0, 0.0, 0.0],
        up: [0.0, 3.0, 0.0],
        depth: 1.0,
        ..Inputs::default()
    };
    let frame = raster_effects(
        &gpu,
        field,
        &[],
        &[9.0, 9.0, 9.0, -100.0],
        &face,
        &reading(&scene),
    );
    let average = |from: f32, to: f32| {
        let pixels: Vec<u32> = (0..40)
            .map(|step| {
                let pixel = at(&frame, from + (to - from) * step as f32 / 40.0, 0.37);
                u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])
            })
            .collect();
        pixels.iter().sum::<u32>() / pixels.len() as u32
    };
    let (seam, open) = (average(0.05, 0.45), average(0.55, 0.95));
    assert!(seam > open + 100, "{seam} {open}");
}

#[test]
fn the_boss_outline_beats_red_and_its_eyes_outshine_its_plate() {
    let Some(gpu) = Gpu::for_fixture("boss") else {
        return;
    };
    let boss = shader!("boss");
    let features = Features {
        outline: true,
        ..Default::default()
    };
    // The outline colour, through the host's bounding, over the frame.
    let outline = |seconds: f32| {
        raster_with_effects(
            &gpu,
            boss,
            &[],
            &[1.0, 0.0],
            &Effects {
                features,
                ..Effects::default()
            },
            |library| {
                format!(
                    "{library}
struct FxVertex {{ @builtin(position) position: vec4<f32> }}
@vertex fn fx_vertex(@builtin(vertex_index) index: u32) -> FxVertex {{
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return FxVertex(vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0));
}}
@fragment fn fx_fragment(vertex: FxVertex) -> @location(0) vec4<f32> {{
    _ = world_material_phase();
    let input = WorldMaterialInput(vec2<f32>(0.5), vec3<f32>(0.0), vec4<f32>(1.0), vec4<f32>(1.0),
        vec4<f32>(0.0), vec4<f32>(0.0), {phase:?}, vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, 1.0));
    return vec4<f32>(world_material_outline(input) / 4.0, 1.0);
}}
",
                    phase = phase(seconds)
                )
            },
        )
    };
    // On the beat (a tenth into it) the outline is brighter than between beats, and red.
    let (beat, rest) = (at(&outline(20.1), 0.5, 0.5), at(&outline(20.6), 0.5, 0.5));
    assert!(
        beat[0] > rest[0] && beat[0] > 3 * beat[1].max(beat[2]),
        "{beat:?} {rest:?}"
    );
    // The eyes burn: the texel rows six and seven of a 64-texel skin at columns 10 and 13.
    let skin = Inputs {
        phase: phase(20.6),
        ..Inputs::default()
    };
    let frame = raster_effects(&gpu, boss, &[], &[1.0, 0.0], &skin, &Effects::default());
    let eye = at(&frame, 10.5 / 64.0, 6.5 / 64.0);
    let plate = at(&frame, 30.5 / 64.0, 30.5 / 64.0);
    assert!(eye[0] == 255 && plate[0] < 100, "{eye:?} {plate:?}");
}

/// Reads `cells` cells of a compute buffer back.
fn cells_of(gpu: &Gpu, buffer: &wgpu::Buffer, cells: u32) -> Vec<[f32; 4]> {
    let bytes = u64::from(cells) * 16;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cells readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, bytes);
    gpu.queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let data = slice.get_mapped_range().unwrap().to_vec();
    bytemuck::cast_slice::<u8, [f32; 4]>(&data).to_vec()
}

#[test]
fn the_storm_keeps_its_particles_alive_around_the_core_and_bursts_them_outward() {
    let Some(gpu) = Gpu::for_fixture("storm step") else {
        return;
    };
    let step = include_str!(
        "../../../../../examples/experiences/material-fx/client/shaders/storm_step.wgsl"
    );
    let cells = 256;
    // The fixture's frames advance the clock a second each; no burst.
    let calm = cells_of(
        &gpu,
        &run_compute(&gpu, step, cells, &[-100.0, 60.0], 40),
        cells,
    );
    assert!(
        calm.iter().all(|c| c[3] > 0.0 && c[3] <= 1.0),
        "every particle lives"
    );
    let radius = |c: &[f32; 4]| (c[0] * c[0] + c[2] * c[2]).sqrt();
    assert!(calm.iter().all(|c| radius(c) <= 4.0 && c[1].abs() <= 4.0));
    let spread = calm.iter().map(radius).fold(0.0_f32, f32::max);
    assert!(spread > 0.8, "the particles ring the core: {spread}");
    // Bursting from the start, they fly further out.
    let burst = cells_of(
        &gpu,
        &run_compute(&gpu, step, cells, &[0.0, 60.0], 3),
        cells,
    );
    let still = cells_of(
        &gpu,
        &run_compute(&gpu, step, cells, &[-100.0, 60.0], 3),
        cells,
    );
    let mean = |cells: &[[f32; 4]]| cells.iter().map(radius).sum::<f32>() / cells.len() as f32;
    assert!(
        mean(&burst) > mean(&still),
        "{} {}",
        mean(&burst),
        mean(&still)
    );
}

#[test]
fn the_fire_spreads_along_the_bottom_climbs_and_goes_out_leaving_char() {
    let Some(gpu) = Gpu::for_fixture("fire step") else {
        return;
    };
    let step = include_str!(
        "../../../../../examples/experiences/material-fx/client/shaders/fire_step.wgsl"
    );
    let (width, height) = (32_u32, 16_u32);
    let lit = cells_of(
        &gpu,
        &run_compute(&gpu, step, width * height, &[0.0, width as f32, 7.0], 2),
        width * height,
    );
    let heat = |cells: &[[f32; 4]], x: u32, y: u32| cells[(y * width + x) as usize][0];
    assert!(heat(&lit, 0, 0) > 0.9, "the ignition burns");
    assert!(heat(&lit, 0, 1) > 0.5, "heat rises");
    assert!(
        heat(&lit, width - 1, height - 1) == 0.0,
        "the far corner is cold"
    );
    // Given time it spreads sideways along the bottom.
    let spread = cells_of(
        &gpu,
        &run_compute(&gpu, step, width * height, &[0.0, width as f32, 7.0], 300),
        width * height,
    );
    let burning = (0..width).filter(|&x| heat(&spread, x, 0) > 0.3).count();
    assert!(burning > 4, "{burning}");
    assert!(
        spread.iter().any(|cell| cell[1] == 1.0),
        "burnt texels char"
    );
    // The firewall material draws the hottest texels in the flame palette, emissive.
    let fire = shader!("firewall");
    let buffer = run_compute(&gpu, step, width * height, &[0.0, width as f32, 7.0], 2);
    let wall = Inputs {
        origin: [-1.0, -0.5, 0.0],
        across: [2.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        ..Inputs::default()
    };
    let frame = raster_effects(
        &gpu,
        fire,
        &[],
        &[0.0, 0.0, 0.0, 0.0, 2.0, 1.0],
        &wall,
        &Effects {
            features: Features {
                cells: true,
                ..Default::default()
            },
            cells: Some(&buffer),
            ..Effects::default()
        },
    );
    let (flame, stone) = (at(&frame, 0.5 / 32.0, 0.5 / 16.0), at(&frame, 0.9, 0.9));
    assert!(flame[0] == 255 && flame[1] > 150, "{flame:?}");
    assert!(
        stone[0] < 150 && stone[0].abs_diff(stone[1]) < 20,
        "{stone:?}"
    );
}
