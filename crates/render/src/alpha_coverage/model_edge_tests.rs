use super::*;

const CASE_COUNT: u32 = 4;
const CELL_SIDE: u32 = SNAPSHOT_SIDE / CASE_COUNT;
// Past every production group-0 binding, which the composed shader also declares.
const EDGE_CASES_BINDING: u32 = 27;
const TILE_SIDE: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Coverage {
    None,
    Partial,
    Full,
}

/// One coverage lookup on a quad edge: `v` in tile units, and whether the quad's UVs wrap.
struct EdgeCase {
    v: f32,
    wrapped: bool,
    coverage: Coverage,
}

/// A tile that is transparent everywhere except its opaque bottom row, like flame artwork.
fn bottom_row_tile(gpu: &Gpu) -> wgpu::TextureView {
    let texels: Vec<_> = (0..TILE_SIDE * TILE_SIDE)
        .flat_map(|pixel| {
            let bottom = pixel / TILE_SIDE == TILE_SIDE - 1;
            [255, 0, 0, if bottom { 255 } else { 0 }]
        })
        .collect();
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("bottom-row cutout tile"),
                size: wgpu::Extent3d {
                    width: TILE_SIDE,
                    height: TILE_SIDE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &texels,
        )
        .create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
}

fn edge_cases() -> [EdgeCase; CASE_COUNT as usize] {
    [
        // A single-tile quad's top edge must not borrow the opposite (bottom) row.
        EdgeCase {
            v: 0.0,
            wrapped: false,
            coverage: Coverage::None,
        },
        // Its bottom edge keeps the opaque row instead of the transparent top row.
        EdgeCase {
            v: 1.0,
            wrapped: false,
            coverage: Coverage::Full,
        },
        // Wrapped quads repeat the tile, so an interior seam still meets the next tile's top.
        EdgeCase {
            v: 1.0,
            wrapped: true,
            coverage: Coverage::Partial,
        },
        EdgeCase {
            v: 0.5,
            wrapped: false,
            coverage: Coverage::None,
        },
    ]
}

/// Draws the production model cutout coverage for each edge case, one column per case.
fn edge_raster(gpu: &Gpu, enhanced: bool) -> Vec<u8> {
    let reference = crate::material_shader::gpu_texture_ref(
        assets::TextureRef::new(0, 0).unwrap(),
        [TILE_SIDE; 2],
        TILE_SIDE,
    );
    let words: Vec<_> = edge_cases()
        .iter()
        .flat_map(|case| [reference, case.v.to_bits(), u32::from(case.wrapped), 0])
        .collect();
    let data = gpu.words(&words, wgpu::BufferUsages::STORAGE);
    let views: [_; assets::MAX_TEXTURE_PAGES] = std::array::from_fn(|_| bottom_row_tile(gpu));
    let sampler = gpu.device.create_sampler(&Default::default());
    let mut bindings = Vec::new();
    for (page, view) in views.iter().enumerate() {
        bindings.push(wgpu::BindGroupEntry {
            binding: if enhanced {
                crate::material_shader::SAMPLED_TEXTURE_BINDINGS[page]
            } else {
                crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[page]
            },
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    for binding in [6, crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING] {
        bindings.push(wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::Sampler(&sampler),
        });
    }
    bindings.push(wgpu::BindGroupEntry {
        binding: EDGE_CASES_BINDING,
        resource: data.as_entire_binding(),
    });
    let definitions: &[&str] = if enhanced {
        &[SHADER_DEF, "ENHANCED"]
    } else {
        &[SHADER_DEF]
    };
    let mut source = crate::shader_source::standalone(include_str!("../model.wesl"), definitions);
    source.push_str(&format!(
        r#"
@group(0) @binding({EDGE_CASES_BINDING}) var<storage, read> edge_cases: array<vec4<u32>>;
@vertex fn edge_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {{
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0);
}}
@fragment fn edge_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {{
    let c = edge_cases[u32(position.x) / {CELL_SIDE}u];
    var input: VertexOutput;
    input.uv = vec2(0.5, bitcast<f32>(c.y));
    input.current_texture = c.x;
    input.visibility.x = select(MODEL_VISIBLE | MODEL_UNIT_TILE, MODEL_VISIBLE, c.z != 0u);
    // Half a texel of footprint crosses the edge while the colour sample stays inside it.
    let dx = vec2(0.0);
    let dy = vec2(0.0, 0.5 / {TILE_SIDE}.0);
    let sampled = sample_model_ref(input, c.x, dx, dy);
    let cutout = model_cutout(input, c.x, dx, dy, sampled);
    return vec4(1.0, 0.0, 0.0, cutout.coverage);
}}
"#
    ));
    gpu.render_with_state(
        &source,
        "edge_vertex",
        &[Draw {
            fragment: "edge_fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }],
        RasterState {
            clear_color: wgpu::Color::BLACK,
            multisample: wgpu::MultisampleState {
                count: 4,
                alpha_to_coverage_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

#[test]
fn single_tile_model_cutout_edges_do_not_wrap_to_the_opposite_row() {
    let Some(gpu) =
        Gpu::for_fixture("single_tile_model_cutout_edges_do_not_wrap_to_the_opposite_row")
    else {
        return;
    };
    for enhanced in [false, true] {
        let frame = edge_raster(&gpu, enhanced);
        let red = |index: usize| {
            let row = SNAPSHOT_SIDE as usize / 2;
            let column = index * CELL_SIDE as usize + CELL_SIDE as usize / 2;
            frame[(row * SNAPSHOT_SIDE as usize + column) * 4]
        };
        for (index, case) in edge_cases().iter().enumerate() {
            let actual = match red(index) {
                0 => Coverage::None,
                255 => Coverage::Full,
                _ => Coverage::Partial,
            };
            assert_eq!(
                actual, case.coverage,
                "Enhanced={enhanced} v={} wrapped={}: cutout coverage must stay on the quad's own texels",
                case.v, case.wrapped
            );
        }
    }
}
