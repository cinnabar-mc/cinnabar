use super::{Page, raster};
use render_model::{UiRenderVertex, UiTexturePage};

#[test]
fn inventory_texture_vertices_interpolate_normalized_short_coordinates() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("inventory texture vertex coordinates")
    else {
        return;
    };
    let cases: [([f32; 6], [u32; 2]); 5] = [
        ([2., 6., 4., 2., 16., 16.], [24576, 32768]),
        ([0., 16., 0., 0., 16., 16.], [0, 65535]),
        ([3. / 4096., 5. / 4096., 0., 0., 16., 16.], [3, 5]),
        ([-1., 17., 0., 0., 16., 16.], [0, 65535]),
        ([8., 24., 8., 8., 32., 64.], [32768, 32768]),
    ];
    let input = gpu.words(
        &cases
            .iter()
            .flat_map(|(values, _)| values.map(f32::to_bits))
            .collect::<Vec<_>>(),
        wgpu::BufferUsages::STORAGE,
    );
    let shader = format!(
        "const STYLE_UNORM16_UV: u32 = {}u;\n{}\n@group(0) @binding(0) var<storage, read> input: array<f32>; @group(0) @binding(1) var<storage, read_write> output: array<vec4<u32>>; @compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) i: vec3<u32>) {{ let p = i.x * 6u; output[i.x] = bitcast<vec4<u32>>(ui_texture_vertex(vec2(input[p], input[p+1u]), vec2(input[p+2u], input[p+3u]), STYLE_UNORM16_UV, vec2(input[p+4u], input[p+5u]))); }}",
        render_api::UI_STYLE_UNORM16_UV,
        include_str!("../../ui_texture_vertex.wgsl"),
    );
    let actual = gpu.compute_readback(
        &shader,
        &[wgpu::BindGroupEntry {
            binding: 0,
            resource: input.as_entire_binding(),
        }],
        1,
        cases.len(),
        cases.len() as u32,
    );
    for ((values, packed), actual) in cases.into_iter().zip(actual) {
        let expected =
            [packed[0] as f32 / 65535., packed[1] as f32 / 65535., 0., 0.].map(f32::to_bits);
        assert_eq!(actual, expected, "texture vertex {values:?}");
    }
}

#[test]
fn opaque_terrain_tints_round_in_gamma_space() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("opaque inventory tint bytes") else {
        return;
    };
    let tint = [83, 137, 197, 255];
    let mut quad = vertices(16.);
    for vertex in &mut quad {
        vertex.color = tint;
    }
    let pixels: Vec<u8> = (0..=255)
        .flat_map(|value| [value, value, value, 255])
        .collect();
    let actual = raster(
        &gpu,
        &quad,
        None,
        &[0, 1, 2, 0, 2, 3],
        Page {
            pixels: &pixels,
            size: [16; 2],
            coverage: false,
            mips: None,
        },
        render_api::UI_STYLE_TERRAIN_MIPS,
    );
    for value in 0..=255usize {
        let pixel = ((20 + value / 16) * 256 + 20 + value % 16) * 4;
        for channel in 0..3 {
            let expected = ((value as u16 * u16::from(tint[channel]) + 127) / 255) as u8;
            assert_eq!(
                actual[pixel + channel],
                expected,
                "source {value}, tint {}",
                tint[channel]
            );
        }
        assert_eq!(actual[pixel + 3], 255);
    }
}

#[test]
fn partial_terrain_texels_weight_tint_without_losing_coverage() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("partial inventory tint coverage") else {
        return;
    };
    let tint = [83, 137, 197, 128];
    let mut quad = vertices(16.);
    for vertex in &mut quad {
        vertex.color = tint;
    }
    let pixels: Vec<u8> = (0..=255).flat_map(|alpha| [120, 180, 240, alpha]).collect();
    for terrain in [false, true] {
        let actual = raster(
            &gpu,
            &quad,
            None,
            &[0, 1, 2, 0, 2, 3],
            Page {
                pixels: &pixels,
                size: [16; 2],
                coverage: false,
                mips: None,
            },
            if terrain {
                render_api::UI_STYLE_TERRAIN_MIPS
            } else {
                0
            },
        );
        for alpha in 0..=255usize {
            let pixel = ((20 + alpha / 16) * 256 + 20 + alpha % 16) * 4;
            let source_alpha = alpha as f64 / 255.;
            let coverage = source_alpha * f64::from(tint[3]) / 255.;
            for (channel, source) in [120., 180., 240.].into_iter().enumerate() {
                let tint = f64::from(tint[channel]) / 255.;
                let shade = if terrain {
                    1. + (tint - 1.) * source_alpha
                } else {
                    tint
                };
                let expected = (source * shade * coverage
                    + f64::from(actual[channel]) * (1. - coverage))
                    .round() as u8;
                assert!(
                    actual[pixel + channel].abs_diff(expected) <= 1,
                    "terrain {terrain}, alpha {alpha}, channel {channel}: {} != {expected}",
                    actual[pixel + channel],
                );
            }
            let expected_alpha =
                (coverage * 255. + f64::from(actual[3]) * (1. - coverage)).round() as u8;
            assert!(actual[pixel + 3].abs_diff(expected_alpha) <= 1);
        }
    }
}

#[test]
fn half_precision_coordinates_keep_even_ties_subnormals_and_signs() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("half precision UI coordinates") else {
        return;
    };
    let cases: [(f32, f32); 12] = [
        (0., 0.),
        (1., 1.),
        (1. + 1. / 2048., 1.),
        (1. + 3. / 2048., 1. + 2. / 1024.),
        (129.071_06, 129.125),
        (129.284_21, 129.25),
        (1. / 33_554_432., 0.),
        (3. / 33_554_432., 1. / 8_388_608.),
        (1. / 16384. - 1. / 33_554_432., 1. / 16384.),
        (65504., 65504.),
        (65520., f32::INFINITY),
        (f32::MAX, f32::INFINITY),
    ];
    let cases: Vec<_> = cases
        .into_iter()
        .flat_map(|(value, expected)| [(value, expected), (-value, -expected)])
        .collect();
    let input = gpu.words(
        &cases
            .iter()
            .map(|(value, _)| value.to_bits())
            .collect::<Vec<_>>(),
        wgpu::BufferUsages::STORAGE,
    );
    let source = format!(
        "{}\n@group(0) @binding(0) var<storage, read> input: array<u32>; @group(0) @binding(1) var<storage, read_write> output: array<vec4<u32>>; @compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) i: vec3<u32>) {{ output[i.x] = vec4(bitcast<u32>(ui_half_float(bitcast<f32>(input[i.x]))), 0u, 0u, 0u); }}",
        include_str!("../../ui_half_float.wgsl"),
    );
    let actual = gpu.compute_readback(
        &source,
        &[wgpu::BindGroupEntry {
            binding: 0,
            resource: input.as_entire_binding(),
        }],
        1,
        cases.len(),
        cases.len() as u32,
    );
    for ((value, expected), actual) in cases.into_iter().zip(actual) {
        assert_eq!(actual[0], expected.to_bits(), "input {value}");
    }
}

#[test]
fn projected_item_vertices_use_half_precision_before_rasterization() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("projected item coordinates") else {
        return;
    };
    let positions = [
        [129.071_06, 129.284_21],
        [136.142_14, 132.819_75],
        [129.071_06, 136.355_29],
        [122.0, 132.819_75],
    ];
    let rounded = [
        [129.125, 129.25],
        [136.125, 132.875],
        [129.125, 136.375],
        [122.0, 132.875],
    ];
    let mut projected = vertices(16.0);
    for (vertex, position) in projected.iter_mut().zip(positions) {
        vertex.position = position;
    }
    let mut reference = projected;
    for (vertex, position) in reference.iter_mut().zip(rounded) {
        vertex.position = position;
    }
    let pixels = (0..16)
        .flat_map(|y| (0..16).flat_map(move |x| [x * 16, y * 16, 127, 255]))
        .collect::<Vec<_>>();
    let draw = |quad: &[UiRenderVertex], flags| {
        raster(
            &gpu,
            quad,
            None,
            &[0, 1, 2, 0, 2, 3],
            Page {
                pixels: &pixels,
                size: [16; 2],
                coverage: false,
                mips: None,
            },
            flags,
        )
    };
    let expected = draw(&reference, 0);
    assert!(
        draw(&projected, 0) != expected,
        "the fixture exposes subpixel differences"
    );
    let actual = draw(&projected, render_api::UI_STYLE_HALF_FLOAT_XY);
    assert!(
        actual == expected,
        "projected item coordinates must round before triangle interpolation: first delta {:?}, differing pixels {}",
        actual
            .chunks_exact(4)
            .zip(expected.chunks_exact(4))
            .enumerate()
            .find(|(_, (a, b))| a != b),
        actual
            .chunks_exact(4)
            .zip(expected.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count(),
    );
}

/// Projects the source onto a square with the requested physical footprint.
fn vertices(side: f32) -> [UiRenderVertex; 4] {
    [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|point| UiRenderVertex {
        position: [20. + point[0] * side, 20. + point[1] * side],
        uv: point.map(|value| value * 16.),
        clip_z: 0.,
        clip_w: 1.,
        color: [255; 4],
        style_flags: 0,
        alpha_cutoff: -1.,
        model_light: 1.,
        overlay_color: [0.; 4],
    })
}

#[test]
fn minified_inventory_surfaces_average_source_texels_without_blurring_magnified_pixels() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("inventory texture minification") else {
        return;
    };
    let pixels = (0..16)
        .flat_map(|y| {
            (0..16).flat_map(move |x| {
                let value = if (x + y) % 2 == 0 { 0 } else { 255 };
                [value, value, value, 255]
            })
        })
        .collect::<Vec<_>>();
    let source = UiTexturePage::owned([16; 2], pixels.into())
        .unwrap()
        .with_terrain_mips()
        .unwrap();
    let average = source.mip_levels().nth(2).unwrap().1[..4].to_vec();
    let reference = average.repeat(16 * 16);
    let indices = [0, 1, 2, 0, 2, 3];
    let draw = |side, page, flags| raster(&gpu, &vertices(side), None, &indices, page, flags);
    let source_page = || Page {
        pixels: source.pixels(),
        size: source.dimensions(),
        coverage: false,
        mips: Some(&source),
    };
    let expected = draw(
        4.,
        Page {
            pixels: &reference,
            size: [16; 2],
            coverage: false,
            mips: None,
        },
        0,
    );
    assert!(draw(4., source_page(), 0) != expected);
    assert!(
        draw(4., source_page(), render_api::UI_STYLE_TERRAIN_MIPS) == expected,
        "a four-to-one surface must sample its averaged source level",
    );
    assert!(
        draw(64., source_page(), render_api::UI_STYLE_TERRAIN_MIPS) == draw(64., source_page(), 0),
        "magnification keeps original nearest-filtered texels",
    );
}

#[test]
fn minified_cutouts_keep_partial_coverage() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("inventory cutout minification") else {
        return;
    };
    let pixels = (0..16 * 16)
        .flat_map(|pixel| [255, 255, 255, if pixel % 4 == 0 { 255 } else { 0 }])
        .collect::<Vec<_>>();
    let source = UiTexturePage::owned([16; 2], pixels.into())
        .unwrap()
        .with_terrain_mips()
        .unwrap();
    let average = source.mip_levels().nth(2).unwrap().1[..4].repeat(16 * 16);
    let mut quad = vertices(4.);
    for vertex in &mut quad {
        vertex.alpha_cutoff = assets::gui_item::GUI_BLOCK_SAMPLE_ALPHA_THRESHOLD;
    }
    let indices = [0, 1, 2, 0, 2, 3];
    let reference = raster(
        &gpu,
        &quad,
        None,
        &indices,
        Page {
            pixels: &average,
            size: [16; 2],
            coverage: false,
            mips: None,
        },
        0,
    );
    let actual = raster(
        &gpu,
        &quad,
        None,
        &indices,
        Page {
            pixels: source.pixels(),
            size: source.dimensions(),
            coverage: false,
            mips: Some(&source),
        },
        render_api::UI_STYLE_TERRAIN_MIPS,
    );
    assert!(actual == reference);
    for vertex in &mut quad {
        vertex.alpha_cutoff = f32::from(assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD) / 255.;
    }
    assert!(
        raster(
            &gpu,
            &quad,
            None,
            &indices,
            Page {
                pixels: source.pixels(),
                size: source.dimensions(),
                coverage: false,
                mips: Some(&source)
            },
            render_api::UI_STYLE_TERRAIN_MIPS,
        ) != reference,
        "a source-texel cutoff erases minified coverage"
    );
}
