//! The retained-drawing material showcase's shaders (examples/mods/material-showcase), rastered
//! through the host's drawing-material module over a full-frame surface and checked for the
//! pixels that tell their effects apart: the beam's glowing core and travelling pulses, and the
//! outline's marching dashes and cycling colour.

use crate::gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};

/// One showcase shader, read from the mod.
macro_rules! shader {
    ($name:literal) => {
        include_str!(concat!(
            "../../../../examples/mods/material-showcase/shaders/",
            $name,
            ".wgsl"
        ))
    };
}

/// The host module over a full-frame surface: uv spans the frame, the line distance runs four
/// blocks across it, the vertex colour is `colour` and the parameters are `params`.
fn source(shade: &str, colour: [f32; 4], params: [f32; 4]) -> String {
    let [r, g, b, a] = colour;
    let [p0, p1, p2, p3] = params;
    format!(
        "{}
@vertex fn fx_vertex(@builtin(vertex_index) index: u32) -> MaterialInput {{
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    // uv.y runs up the frame from 0 at its bottom edge, as up a beam.
    return MaterialInput(vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0), corner,
        vec3<f32>(0.0), vec4<f32>({r:?}, {g:?}, {b:?}, {a:?}), corner.x * 4.0,
        vec4<f32>({p0:?}, {p1:?}, {p2:?}, {p3:?}), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
}}
",
        mod_render::material::module(shade)
    )
}

/// Rasters `shade` at animation phase `phase`; returns RGBA8 rows.
fn raster(gpu: &Gpu, shade: &str, colour: [f32; 4], params: [f32; 4], phase: f32) -> Vec<u8> {
    mod_render::material::compose(shade).expect("the showcase shader passes the sandbox");
    let mut view = [0.0_f32; 24];
    view[20] = phase;
    let view = gpu.buffer(&view, wgpu::BufferUsages::UNIFORM);
    gpu.render(
        &source(shade, colour, params),
        "fx_vertex",
        &[Draw {
            fragment: "material_fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            }],
            blend: None,
            write_depth: false,
        }],
    )
}

/// The pixel at `(u, v)` of a frame, `v` measured from the top.
fn at(frame: &[u8], u: f32, v: f32) -> [u8; 4] {
    let side = SNAPSHOT_SIDE as f32;
    let (x, y) = ((u * side) as usize, (v * side) as usize);
    let index = (y * SNAPSHOT_SIDE as usize + x) * 4;
    frame[index..index + 4].try_into().unwrap()
}

fn brightness(pixel: [u8; 4]) -> u32 {
    pixel[..3].iter().map(|&c| u32::from(c)).sum()
}

#[test]
fn the_beam_glows_at_its_core_fades_upwards_and_carries_moving_pulses() {
    let Some(gpu) = Gpu::for_fixture("drawing material beam") else {
        return;
    };
    let orange = [1.0, 0.35, 0.1, 1.0];
    let params = [3.0, 1.0, 4.0, 0.0];
    let frame = raster(&gpu, shader!("beam"), orange, params, 0.0);
    let (core, edge) = (at(&frame, 0.5, 0.9), at(&frame, 0.02, 0.9));
    assert!(
        brightness(core) > brightness(edge) + 200,
        "{core:?} {edge:?}"
    );
    assert!(
        core[0] > core[2],
        "the core glows in the beam's colour: {core:?}"
    );
    let (bottom, top) = (at(&frame, 0.5, 0.95), at(&frame, 0.5, 0.02));
    assert!(brightness(bottom) > brightness(top), "{bottom:?} {top:?}");
    let column: Vec<u32> = (0..64)
        .map(|row| brightness(at(&frame, 0.5, 0.5 + row as f32 / 160.0)))
        .collect();
    assert!(
        column.iter().max().unwrap() > &(column.iter().min().unwrap() + 60),
        "pulses brighten bands of the beam: {column:?}"
    );
    let later = raster(&gpu, shader!("beam"), orange, params, 1.0 / (1024.0 * 4.0));
    let moved = (0..64)
        .filter(|row| {
            let v = 0.5 + *row as f32 / 160.0;
            brightness(at(&frame, 0.5, v)).abs_diff(brightness(at(&later, 0.5, v))) > 40
        })
        .count();
    assert!(moved > 8, "the pulses travel with the phase: {moved}");
}

#[test]
fn the_outline_marches_dashes_and_cycles_its_colour_along_the_line() {
    let Some(gpu) = Gpu::for_fixture("drawing material outline") else {
        return;
    };
    let params = [4.0, 2.0, 3.0, 0.0];
    let frame = raster(&gpu, shader!("outline"), [1.0; 4], params, 0.0);
    let row: Vec<[u8; 4]> = (0..256)
        .map(|column| at(&frame, column as f32 / 256.0, 0.5))
        .collect();
    let gaps = row.iter().filter(|pixel| pixel[3] == 0).count();
    let dashes = row.iter().filter(|pixel| pixel[3] == 255).count();
    assert!(
        gaps > 40 && dashes > 100,
        "dashes and gaps alternate: {gaps} {dashes}"
    );
    let lit: Vec<&[u8; 4]> = row.iter().filter(|pixel| pixel[3] == 255).collect();
    let reddest = lit
        .iter()
        .max_by_key(|pixel| i32::from(pixel[0]) - i32::from(pixel[2]))
        .unwrap();
    let bluest = lit
        .iter()
        .max_by_key(|pixel| i32::from(pixel[2]) - i32::from(pixel[0]))
        .unwrap();
    assert!(
        reddest[0] > reddest[2] + 80 && bluest[2] > bluest[0] + 80,
        "the colour cycles along the line: {reddest:?} {bluest:?}"
    );
    let marched = raster(
        &gpu,
        shader!("outline"),
        [1.0; 4],
        params,
        1.0 / (1024.0 * 8.0),
    );
    let changed = (0..256)
        .filter(|column| {
            at(&frame, *column as f32 / 256.0, 0.5)[3]
                != at(&marched, *column as f32 / 256.0, 0.5)[3]
        })
        .count();
    assert!(changed > 20, "the dashes march with the phase: {changed}");
}
