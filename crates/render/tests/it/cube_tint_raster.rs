//! Uniform cube tints may skip fragment lookup; mixed and swamp grass retain per-block colour.
use crate::{chunk_constants, gpu_snapshot, material_shader, shader_source, tint_bake_plan};
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu};
use meshing::{Face, PackedBiomeRecord, PackedQuad};
use std::sync::Arc;
use world::{DecodedBiomeColumn, RawBiomeIds};

const COLUMNS: usize = 6;
const ROWS: usize = 6;
const CASES: usize = COLUMNS * ROWS;
/// Group-0 slot the fixtures give the lightmap: the tint cache's, free in the uncached layout.
const LIGHTMAP_BINDING: u32 = material_shader::TERRAIN_TINT_CACHE_BINDING;
/// The lightmap's slot beside the tint cache.
const CACHED_LIGHTMAP_BINDING: u32 = material_shader::TERRAIN_TINT_CACHE_BINDING + 1;

/// Encodes normal, swamp/seasonal and mixed descriptors through the production biome packer.
fn records() -> [PackedBiomeRecord; 3] {
    let storages: [_; 3] = std::array::from_fn(|index| {
        DecodedBiomeColumn::decode(
            0,
            1,
            &[1, (index * 2) as u8],
            &RawBiomeIds { default_biome: 0 },
        )
        .storage(0)
        .expect("fixture biome storage")
    });
    std::array::from_fn(|row| {
        let neighbours = std::array::from_fn(|index| {
            Some(Arc::clone(
                &storages[if row == 2 { index % 3 } else { row }],
            ))
        });
        let record = PackedBiomeRecord::from_neighbourhood(&neighbours, |id| id);
        assert_eq!(record.uniform_tint_index().is_none(), row == 2);
        record
    })
}

/// Makes seasonal exposure, foliage species and spatial swamp noise visibly different.
fn tint_words() -> Vec<u32> {
    let mut words = Vec::new();
    for index in 0..3 + assets::TINT_MAP_SIZE {
        let packed = |phase: u32| {
            let components = [
                index * 173 + phase,
                index * 47 + phase * 3,
                index * 23 + phase * 7,
            ];
            components
                .into_iter()
                .enumerate()
                .fold(0, |word, (channel, value)| {
                    word | ((value % 900 + 100) << (channel * 10))
                })
        };
        let flags = if index == 1 {
            assets::BIOME_TINT_FLAG_SWAMP_GRASS | assets::BIOME_TINT_FLAG_SEASONAL_FOLIAGE
        } else {
            0
        };
        words.extend([
            packed(31),
            packed(79),
            packed(131),
            packed(191),
            packed(257),
            0,
            flags,
            1.0_f32.to_bits(),
        ]);
        for cell in 0..assets::seasonal_foliage::SEASONAL_FOLIAGE_COUNT {
            words.extend(
                [
                    0.12 + cell as f32 * 0.23,
                    0.7 - cell as f32 * 0.1,
                    0.24 + cell as f32 * 0.11,
                    1.0,
                ]
                .map(f32::to_bits),
            );
        }
    }
    words
}

/// Covers overlays, native leaves, regular foliage and exposed seasonal species.
fn material_flags() -> [u32; COLUMNS] {
    let leaf = assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR | assets::MATERIAL_FLAG_FOLIAGE_TINT;
    let seasonal = leaf | assets::MATERIAL_FLAG_SEASONAL_FOLIAGE;
    [
        assets::MATERIAL_FLAG_GRASS_TINT | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        assets::MATERIAL_FLAG_GRASS_TINT | assets::MATERIAL_FLAG_OVERLAY_MASK,
        leaf | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        seasonal | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        seasonal
            | assets::MATERIAL_FLAG_EXPOSED_FOLIAGE
            | assets::MATERIAL_FLAG_BIRCH_FOLIAGE
            | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        seasonal
            | assets::MATERIAL_FLAG_EXPOSED_FOLIAGE
            | assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE
            | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
    ]
    .map(|flags| flags | assets::MATERIAL_FLAG_TWO_SIDED)
}

/// Ordinary terrain samples the encoded-byte view of the sRGB atlas.
fn atlas(gpu: &Gpu) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cube tint raster atlas"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 2,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    let texels = (0..32)
        .flat_map(|index| {
            [
                80 + (index % 4) * 40,
                210 - (index / 16) * 70,
                64 + (index % 4) * 20,
                [0, 127, 128, 255][index as usize % 4],
            ]
        })
        .collect::<Vec<u8>>();
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16),
            rows_per_image: Some(4),
        },
        texture.size(),
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// Replaces only the colour helper with its complete fragment-tint reference.
fn source(reference: bool) -> String {
    source_with(reference, &[], LIGHTMAP_BINDING)
}

/// [`source`] composed with `definitions`, moving the lightmap into group 0 at `lightmap`.
fn source_with(reference: bool, definitions: &[&str], lightmap: u32) -> String {
    let mut module: wesl::syntax::TranslationUnit =
        crate::material_shader::source(include_str!("../../src/chunk.wesl"))
            .parse()
            .expect("chunk WESL parses");
    if reference {
        let mut replacement: wesl::syntax::TranslationUnit =
            REFERENCE_SHADE.parse().expect("reference helper parses");
        let shade = module
            .global_declarations
            .iter_mut()
            .find(|declaration| {
                let wesl::syntax::GlobalDeclaration::Function(function) = declaration.node() else {
                    return false;
                };
                function.ident.to_string() == "shade_cube"
            })
            .expect("cube shade helper");
        *shade = replacement.global_declarations.remove(0);
    }
    let mut source = shader_source::standalone(&module.to_string(), definitions).replace(
        "@group(1) @binding(0)",
        &format!("@group(0) @binding({lightmap})"),
    );
    source.push_str(
        &VERTEX
            .replace(
                "INDICES",
                &chunk_constants::STATIC_QUAD_INDICES
                    .map(|index| format!("{index}u"))
                    .join(", "),
            )
            .replace("COLUMNS", &format!("{COLUMNS}.0"))
            .replace("ROWS", &format!("{ROWS}.0")),
    );
    source
}

/// Follows calls through nested control flow without depending on source spelling or layout.
fn calls(module: &naga::Module, statement: &naga::Statement, name: &str) -> bool {
    let block_calls =
        |block: &naga::Block| block.iter().any(|statement| calls(module, statement, name));
    match statement {
        naga::Statement::Call { function, .. } => {
            let function = &module.functions[*function];
            function.name.as_deref() == Some(name) || block_calls(&function.body)
        }
        naga::Statement::Block(block) => block_calls(block),
        naga::Statement::If { accept, reject, .. } => block_calls(accept) || block_calls(reject),
        naga::Statement::Switch { cases, .. } => cases.iter().any(|case| block_calls(&case.body)),
        naga::Statement::Loop {
            body, continuing, ..
        } => block_calls(body) || block_calls(continuing),
        _ => false,
    }
}

#[test]
fn uniform_cube_tints_return_before_fragment_biome_work() {
    let source = shader_source::standalone(include_str!("../../src/chunk.wesl"), &[]);
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let helper = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("ordinary_cube_tint_gamma"))
        .expect("ordinary cubes must reuse their admitted vertex tint")
        .1;
    let (gate, accept) = helper
        .body
        .iter()
        .enumerate()
        .find_map(|(index, statement)| {
            if let naga::Statement::If { accept, .. } = statement {
                Some((index, accept))
            } else {
                None
            }
        })
        .expect("cached tint must return before the fallback lookup");
    assert!(
        !helper.body.iter().take(gate).any(|statement| calls(
            &module,
            statement,
            "blended_biome_tint"
        )),
        "the cached branch must not follow an unconditional biome query"
    );
    assert!(
        accept
            .iter()
            .any(|statement| matches!(statement, naga::Statement::Return { .. }))
    );
    assert!(
        !accept
            .iter()
            .any(|statement| calls(&module, statement, "blended_biome_tint"))
    );
    assert!(
        helper
            .body
            .iter()
            .any(|statement| calls(&module, statement, "blended_biome_tint")),
        "mixed records retain the shared blender"
    );
    let vertex = module
        .entry_points
        .iter()
        .find(|entry| entry.stage == naga::ShaderStage::Vertex)
        .unwrap();
    assert!(vertex.function.body.iter().any(|statement| calls(
        &module,
        statement,
        "uniform_biome_tint_gamma"
    )));
    let result = vertex.function.result.as_ref().unwrap();
    let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        panic!("cube vertex outputs");
    };
    assert!(members.iter().any(|member| matches!(
        member.binding,
        Some(naga::Binding::Location {
            location: 12,
            interpolation: Some(naga::Interpolation::Flat),
            ..
        })
    )));
}

#[test]
fn enhanced_and_shadow_cube_vertices_do_not_resolve_biome_tint() {
    for definitions in [
        &["ENHANCED"][..],
        &["ENHANCED_SHADOW"][..],
        &["OPAQUE_OVERDRAW"][..],
    ] {
        let source = shader_source::standalone(include_str!("../../src/chunk.wesl"), definitions);
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        let vertex = module
            .entry_points
            .iter()
            .find(|entry| entry.stage == naga::ShaderStage::Vertex)
            .unwrap();
        for name in ["uniform_biome_tint_gamma", "blended_biome_tint"] {
            assert!(
                !vertex
                    .function
                    .body
                    .iter()
                    .any(|statement| calls(&module, statement, name)),
                "{definitions:?} vertex reaches {name}"
            );
        }
    }
}

#[test]
fn uniform_cube_pixels_match_fragment_reference_and_mixed_fallback() {
    render_fixture(false);
}

#[test]
fn uniform_cube_cache_admits_only_position_independent_descriptors() {
    render_fixture(true);
}

/// Production-encoded terrain buffers drawing one independently visible cell per case.
struct Fixture {
    gpu: Gpu,
    buffers: [wgpu::Buffer; 13],
    native_atlas: wgpu::TextureView,
    sampler: wgpu::Sampler,
    native_sampler: wgpu::Sampler,
}

impl Fixture {
    /// Gives every quad `extent` blocks along its width and height axes on `gpu`.
    fn new(gpu: Gpu, extent: [u8; 2]) -> Self {
        let storage = wgpu::BufferUsages::STORAGE;
        let uniform = wgpu::BufferUsages::UNIFORM;
        let mut biome_words = Vec::new();
        let descriptors = records();
        let mut offsets = descriptors
            .iter()
            .map(|record| {
                let offset = biome_words.len() as u32;
                biome_words.extend(record.words());
                offset
            })
            .collect::<Vec<_>>();
        for invalid_magic in [true, false] {
            let mut words = descriptors[0].words().to_vec();
            if invalid_magic {
                words[0] = 0;
            } else {
                words[1] = u32::MAX - 1;
            }
            offsets.push(biome_words.len() as u32);
            biome_words.extend(words);
        }
        offsets.push(u32::MAX);
        assert_eq!(offsets.len(), ROWS);
        let quads = (0..CASES)
            .map(|index| {
                PackedQuad::new(
                    [0, (index % 8) as u8, 0],
                    Face::ALL[index % COLUMNS],
                    extent[0],
                    extent[1],
                    (index % COLUMNS) as u32,
                )
            })
            .collect::<Vec<_>>();
        let origins = (0..CASES)
            .flat_map(|index| {
                [
                    (-320_i32) as u32,
                    64,
                    (-480_i32) as u32,
                    offsets[index / COLUMNS],
                    0,
                    0,
                    0,
                    0,
                ]
            })
            .collect::<Vec<_>>();
        let materials = material_flags()
            .into_iter()
            .enumerate()
            .flat_map(|(index, flags)| {
                crate::material_records::plain_material(
                    0,
                    flags,
                    if index % 2 == 0 {
                        assets::NO_ANIMATION
                    } else {
                        0
                    },
                )
            })
            .collect::<Vec<_>>();
        let mut atmosphere = [0.0; 40];
        atmosphere[16..19].copy_from_slice(&[0.13, 0.22, 0.31]);
        atmosphere[36..39]
            .copy_from_slice(&render_model::celestial::rgb_to_gamma([0.13, 0.22, 0.31]));
        atmosphere[20] = 64.0;
        let buffers = [
            gpu.buffer(
                &gpu_snapshot::view(Mat4::IDENTITY, Vec3::new(-312.0, 68.0, -472.0)),
                uniform,
            ),
            gpu.words(
                &quads.iter().flat_map(PackedQuad::words).collect::<Vec<_>>(),
                storage,
            ),
            gpu.words(&origins, storage),
            gpu.words(&materials, storage),
            gpu.words(&biome_words, storage),
            gpu.words(&tint_words(), storage),
            gpu.words(&meshing::biome_lattice::query_table_words(), uniform),
            gpu.words(
                &[0, 2, 4, assets::ANIMATION_FLAG_BLEND, 1.0_f32.to_bits()],
                storage,
            ),
            gpu.words(&[0, 1], storage),
            gpu.words(&[1, 0.25_f32.to_bits(), 0, 0], uniform),
            gpu.words(&crate::solid_terrain_raster::lighting_words(CASES), storage),
            gpu.buffer(&atmosphere, uniform),
            gpu.lightmap(bytemuck::cast_slice(
                &render_model::LightmapInputs::default().build(),
            )),
        ];
        let native_atlas = atlas(&gpu);
        let sampler = gpu.device.create_sampler(&Default::default());
        let native_sampler = gpu
            .device
            .create_sampler(&material_shader::native_leaf_sampler_descriptor());
        Self {
            gpu,
            buffers,
            native_atlas,
            sampler,
            native_sampler,
        }
    }

    /// The terrain group-0 entries, with the lightmap at `lightmap`.
    fn bindings(&self, lightmap: u32) -> Vec<wgpu::BindGroupEntry<'_>> {
        let [
            view,
            quads,
            origins,
            materials,
            records,
            tints,
            query_tables,
            animations,
            frames,
            clock,
            streams,
            atmosphere,
            lightmap_buffer,
        ] = &self.buffers;
        let atlas = || wgpu::BindingResource::TextureView(&self.native_atlas);
        [
            (0, view.as_entire_binding()),
            (1, quads.as_entire_binding()),
            (2, origins.as_entire_binding()),
            (3, materials.as_entire_binding()),
            (6, wgpu::BindingResource::Sampler(&self.sampler)),
            (7, records.as_entire_binding()),
            (8, tints.as_entire_binding()),
            (9, animations.as_entire_binding()),
            (10, frames.as_entire_binding()),
            (11, clock.as_entire_binding()),
            (13, streams.as_entire_binding()),
            (15, atmosphere.as_entire_binding()),
            (
                crate::material_shader::BIOME_QUERY_TABLES_BINDING,
                query_tables.as_entire_binding(),
            ),
            (lightmap, lightmap_buffer.as_entire_binding()),
            (material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0], atlas()),
            (material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1], atlas()),
            (material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[2], atlas()),
            (material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[3], atlas()),
            (material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[4], atlas()),
            (
                material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                wgpu::BindingResource::Sampler(&self.native_sampler),
            ),
        ]
        .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource })
        .into()
    }
}

/// Checks either exact old pixels or observable admission without requiring the new varying in both.
fn render_fixture(cache_admission: bool) {
    let Some(gpu) = Gpu::for_fixture("uniform cube tint raster") else {
        return;
    };
    let fixture = Fixture::new(gpu, [16, 8]);
    let gpu = &fixture.gpu;
    let bindings = fixture.bindings(LIGHTMAP_BINDING);
    let draw = |fragment| Draw {
        fragment,
        vertices: 0..CASES as u32 * 6,
        bindings: &bindings,
        blend: None,
        write_depth: true,
    };
    let reference = source(true);
    let candidate = source(false);
    if !cache_admission {
        for fragment in ["fragment", "fragment_solid"] {
            let expected = gpu.render_srgb(&reference, "tint_cube_vertex", &[draw(fragment)]);
            let actual = gpu.render_srgb(&candidate, "tint_cube_vertex", &[draw(fragment)]);
            let mismatched = expected
                .as_chunks::<4>()
                .0
                .iter()
                .zip(actual.as_chunks::<4>().0.iter())
                .filter(|(left, right)| left != right)
                .count();
            assert_eq!(
                mismatched, 0,
                "{fragment}: cached and per-fragment tint must be identical"
            );
            assert!(
                actual
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel != &actual[..4]),
                "fixture must draw coloured terrain"
            );
        }
        return;
    }
    let markers = gpu.render_srgb(
        &format!("{candidate}\n{MARKER}"),
        "tint_cube_vertex",
        &[draw("cache_marker")],
    );
    let side = gpu_snapshot::SNAPSHOT_SIDE as usize;
    for case in 0..CASES {
        let row = case / COLUMNS;
        let column = case % COLUMNS;
        let offset = (((row * 2 + 1) * side / (ROWS * 2)) * side
            + (column * 2 + 1) * side / (COLUMNS * 2))
            * 4;
        let cached = row == 0 || (row == 1 && column >= 2) || row == 4;
        assert_eq!(
            markers[offset + 3],
            if cached { 255 } else { 0 },
            "case {case}: uniform admission and fallback"
        );
    }
}

/// One-block faces of mixed, swamp and invalid records take the vertex blend. The production
/// bake kernel fills their cache entries, and reading them must draw exactly the uncached
/// pixels; a zeroed cache computes in the vertex stage, and a valid entry is what the face draws.
#[test]
fn baked_one_block_tints_draw_the_uncached_blend() {
    let name = "one-block tint cache raster";
    let Some((_, adapter)) =
        Gpu::for_fixture_with_adapter(name, wgpu::Features::empty(), Default::default())
    else {
        return;
    };
    // The cached layout takes a storage slot beyond the default limit; production devices
    // take the adapter's.
    let limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: adapter.limits().max_storage_buffers_per_shader_stage,
        ..Default::default()
    };
    let Some(gpu) = Gpu::for_fixture_with_limits(name, wgpu::Features::empty(), limits) else {
        return;
    };
    let fixture = Fixture::new(gpu, [1, 1]);
    let gpu = &fixture.gpu;
    let draw = |bindings| Draw {
        fragment: "fragment_solid",
        vertices: 0..CASES as u32 * 6,
        bindings,
        blend: None,
        write_depth: true,
    };
    let plain = fixture.bindings(LIGHTMAP_BINDING);
    let expected = gpu.render_srgb(&source(false), "tint_cube_vertex", &[draw(&plain)]);
    assert!(
        expected
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel != &expected[..4])
    );
    let cache = gpu.words(
        &vec![0; CASES * 4],
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
    );
    let mut cached = fixture.bindings(CACHED_LIGHTMAP_BINDING);
    cached.push(wgpu::BindGroupEntry {
        binding: material_shader::TERRAIN_TINT_CACHE_BINDING,
        resource: cache.as_entire_binding(),
    });
    let cached_source = source_with(false, &["TERRAIN_TINT_CACHE"], CACHED_LIGHTMAP_BINDING);
    let render = || gpu.render_srgb(&cached_source, "tint_cube_vertex", &[draw(&cached)]);
    assert!(
        render() == expected,
        "a zeroed cache computes the blend in the vertex stage"
    );
    bake(&fixture, &cache);
    let entries = read_words(gpu, &cache);
    let entries = entries.as_chunks::<4>().0;
    let baked = entries.iter().filter(|entry| entry[3] == 1).count();
    assert!(
        baked >= COLUMNS,
        "every mixed one-block face bakes its blend"
    );
    assert!(
        entries[..COLUMNS].iter().all(|entry| entry == &[0; 4]),
        "uniform records keep their vertex tint"
    );
    assert!(
        render() == expected,
        "baked entries draw the uncached blend"
    );
    let poisoned: Vec<u32> = entries
        .iter()
        .flat_map(|entry| {
            if entry[3] == 1 {
                [0.5_f32.to_bits(), 0.0_f32.to_bits(), 1.0_f32.to_bits(), 1]
            } else {
                *entry
            }
        })
        .collect();
    gpu.queue
        .write_buffer(&cache, 0, bytemuck::cast_slice(&poisoned));
    assert!(render() != expected, "faces draw their valid cache entries");
    // A reused slot whose bake cannot run yet is cleared, and the vertex stage computes it.
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    tint_bake_plan::clear(&mut encoder, &cache, &fixture_jobs(&cache));
    gpu.queue.submit([encoder.finish()]);
    assert!(
        render() == expected,
        "cleared entries draw the uncached blend"
    );
}

/// One job per fixture quad, as the renderer plans an allocation: each quad has its own origin.
fn fixture_jobs(cache: &wgpu::Buffer) -> Vec<tint_bake_plan::Job> {
    let entries = cache.size() / tint_bake_plan::ENTRY_BYTES;
    (0..CASES as u32)
        .filter_map(|quad| tint_bake_plan::job(quad..quad + 1, quad, entries))
        .collect()
}

/// Runs the production bake kernel, layouts and dispatch over every fixture quad into `cache`.
fn bake(fixture: &Fixture, cache: &wgpu::Buffer) {
    let gpu = &fixture.gpu;
    let source = shader_source::composed_prepared_with(
        &tint_bake_plan::shader_source(include_str!("../../src/terrain_tint_bake.wesl")),
        &[],
        Vec::new(),
    );
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain tint bake"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let layout = |entries: &[wgpu::BindGroupLayoutEntry]| {
        gpu.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries,
            })
    };
    let sources_layout = layout(&tint_bake_plan::source_layout_entries());
    let output_layout = layout(&tint_bake_plan::output_layout_entries());
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("terrain tint bake"),
            layout: Some(
                &gpu.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &[Some(&sources_layout), Some(&output_layout)],
                        immediate_size: 0,
                    }),
            ),
            module: &module,
            entry_point: Some("bake"),
            compilation_options: Default::default(),
            cache: None,
        });
    // Seven workgroups per axis: six layers of seven rows reach 42 job slots for 36 jobs, so
    // the overshooting slots must stop at the header's count instead of baking stale jobs.
    let limit = 7;
    let jobs: Vec<_> = fixture_jobs(cache)
        .into_iter()
        .flat_map(|job| tint_bake_plan::split(job, limit))
        .collect();
    assert!(jobs.len() <= tint_bake_plan::reach(limit));
    let capacity = tint_bake_plan::INITIAL_JOB_CAPACITY;
    assert!(jobs.len() < capacity as usize);
    // A persistent buffer from an earlier bake: every slot holds a job naming the wrong origin.
    let stale = tint_bake_plan::job_words(&vec![[0, CASES as u32, 0, 0]; capacity as usize]);
    let job_buffer = gpu.words(
        &stale,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    );
    assert_eq!(
        job_buffer.size(),
        tint_bake_plan::job_buffer_bytes(capacity)
    );
    gpu.queue.write_buffer(
        &job_buffer,
        0,
        bytemuck::cast_slice(&tint_bake_plan::job_words(&jobs)),
    );
    let sources = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &sources_layout,
        entries: &fixture
            .bindings(LIGHTMAP_BINDING)
            .into_iter()
            .filter(|entry| {
                tint_bake_plan::SOURCE_STORAGE_BINDINGS.contains(&entry.binding)
                    || entry.binding == material_shader::BIOME_QUERY_TABLES_BINDING
            })
            .collect::<Vec<_>>(),
    });
    let output = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &output_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: job_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: cache.as_entire_binding(),
            },
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &sources, &[]);
        pass.set_bind_group(1, &output, &[]);
        tint_bake_plan::dispatch(&mut pass, &jobs, limit);
    }
    gpu.queue.submit([encoder.finish()]);
}

/// The blend's palette memo returns the bits the per-sample conversion computes: every block of
/// mixed records whose neighbourhoods include swamp grass, for grass, foliage, seasonal foliage
/// and water.
#[test]
fn memoised_blend_matches_per_sample_conversion() {
    const MEMO_CALL: &str = "memo_tint_gamma(&memo, tint, tint_kind, material_flags, position + vec3<i32>(world_origin))";
    const PER_SAMPLE: &str = "tint_to_gamma(tint_domain_colour(tint, tint_kind, material_flags, position + vec3<i32>(world_origin)))";
    let Some(gpu) = Gpu::for_fixture("memoised biome blend") else {
        return;
    };
    let prepared = material_shader::bind_biome_tables(&meshing::biome_lattice::shader_source(
        include_str!("../../src/biome_tint.wesl"),
    ));
    let module = shader_source::standalone_prepared(&prepared, &[]);
    let start = module
        .find("fn blended_biome_tint(")
        .expect("shared blender");
    let end = start + module[start..].find("\n}\n").expect("blender body") + 3;
    let blender = &module[start..end];
    assert_eq!(
        blender.matches(MEMO_CALL).count(),
        1,
        "reference conversion site"
    );
    let reference = blender
        .replace(
            "fn blended_biome_tint(",
            "fn per_sample_blended_biome_tint(",
        )
        .replace(MEMO_CALL, PER_SAMPLE);
    let source = format!("{module}\n{reference}\n{MEMO_WITNESS}");
    let mut words = Vec::new();
    let offsets: Vec<u32> = swamp_mixed_records()
        .iter()
        .map(|record| {
            let offset = words.len() as u32;
            words.extend(record.words());
            offset
        })
        .collect();
    let flags = [
        assets::MATERIAL_FLAG_GRASS_TINT,
        assets::MATERIAL_FLAG_FOLIAGE_TINT,
        assets::MATERIAL_FLAG_FOLIAGE_TINT | assets::MATERIAL_FLAG_SEASONAL_FOLIAGE,
        assets::MATERIAL_FLAG_WATER_TINT,
    ];
    let cases: Vec<u32> = offsets
        .iter()
        .flat_map(|&offset| flags.map(|flags| [offset, flags]))
        .flatten()
        .collect();
    let storage = wgpu::BufferUsages::STORAGE;
    let records = gpu.words(&words, storage);
    let tints = gpu.words(&tint_words(), storage);
    let tables = gpu.words(
        &meshing::biome_lattice::query_table_words(),
        wgpu::BufferUsages::UNIFORM,
    );
    let case_buffer = gpu.words(&cases, storage);
    let case_count = cases.len() / 2;
    let blocks = 16 * 16 * 16;
    let results = gpu.compute_readback(
        &source,
        &[
            wgpu::BindGroupEntry {
                binding: 7,
                resource: records.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: tints.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: material_shader::BIOME_QUERY_TABLES_BINDING,
                resource: tables.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: MEMO_CASES_BINDING,
                resource: case_buffer.as_entire_binding(),
            },
        ],
        MEMO_OUTPUT_BINDING,
        case_count * blocks,
        (case_count * blocks / 64) as u32,
    );
    let mismatched = results.iter().filter(|result| result[0] != 0).count();
    assert_eq!(
        mismatched, 0,
        "memoised and per-sample blends must be bit-identical"
    );
    assert!(
        results.iter().all(|result| result[3] == 1),
        "every block was evaluated"
    );
    assert!(
        results.iter().any(|result| result[1] != results[0][1]),
        "the records blend distinct tints"
    );
}

/// Mixed records whose neighbourhoods combine plain biomes with the swamp-grass tint.
fn swamp_mixed_records() -> Vec<PackedBiomeRecord> {
    let storages: Vec<_> = (0..3_u8)
        .map(|id| {
            DecodedBiomeColumn::decode(0, 1, &[1, id * 2], &RawBiomeIds { default_biome: 0 })
                .storage(0)
                .expect("fixture biome storage")
        })
        .collect();
    (0..2)
        .map(|shift| {
            let neighbours = std::array::from_fn(|index| {
                Some(Arc::clone(&storages[(index + shift) % storages.len()]))
            });
            let record = PackedBiomeRecord::from_neighbourhood(&neighbours, |id| id / 2);
            assert!(record.uniform_tint_index().is_none());
            record
        })
        .collect()
}

const MEMO_CASES_BINDING: u32 = 40;
const MEMO_OUTPUT_BINDING: u32 = 41;

const MEMO_WITNESS: &str = r#"
@group(0) @binding(40) var<storage, read> memo_cases: array<vec2<u32>>;
@group(0) @binding(41) var<storage, read_write> memo_results: array<vec4<u32>>;

// One invocation per block of each (record, material) case: whether the bits differ, then the
// memoised red bits and the reference red bits.
@compute @workgroup_size(64)
fn witness(@builtin(global_invocation_id) id: vec3<u32>) {
    let fixture = memo_cases[id.x / 4096u];
    let block = id.x % 4096u;
    let local = vec3<f32>(vec3(block / 256u, (block / 16u) % 16u, block % 16u)) + vec3(0.5);
    let origin = vec3(-320.0, 64.0, -480.0);
    let kind = fixture.y & 0x30u;
    let memo = bitcast<vec4<u32>>(blended_biome_tint(kind, fixture.y, fixture.x, local, origin));
    let reference = bitcast<vec4<u32>>(per_sample_blended_biome_tint(kind, fixture.y, fixture.x, local, origin));
    memo_results[id.x] = vec4(u32(any(memo != reference)), memo.x, reference.x, 1u);
}
"#;

/// Copies `buffer` back as words.
fn read_words(gpu: &Gpu, buffer: &wgpu::Buffer) -> Vec<u32> {
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tint cache readback"),
        size: buffer.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, buffer.size());
    gpu.queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    bytemuck::cast_slice(&readback.slice(..).get_mapped_range().unwrap()).to_vec()
}

const VERTEX: &str = r#"
// Keeps real greedy positions and UVs while laying each face in an independently visible cell.
@vertex fn tint_cube_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let indices = array<u32, 6>(INDICES);
    let corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    let quad = index / 6u;
    let corner = indices[index % 6u];
    var out = cube_vertex(quad * 4u + corner, quad);
    let cell = vec2(f32(quad % u32(COLUMNS)), f32(quad / u32(COLUMNS)));
    let position = (cell + corners[corner]) / vec2(COLUMNS, ROWS);
    out.clip_position = vec4(position * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.5, 1.0);
    return out;
}
"#;

const REFERENCE_SHADE: &str = r#"
// Resolves each covered pixel's owning block before the existing native colour transfer.
fn shade_cube(in: VertexOutput, sampled: vec4<f32>, uv_dx: vec2<f32>, uv_dy: vec2<f32>) -> vec4<f32> {
    var tint_gamma = vec3(1.0);
    let tint_kind = in.material_flags & 0x30u;
    if (tint_kind != 0u) {
        tint_gamma = blended_biome_tint_gamma(
            tint_kind,
            in.material_flags,
            in.biome_record,
            in.local_position - quad_shape_normal(in.quad_shape) * 0.001,
            in.world_position - in.local_position,
        ).rgb;
    }
    let native_colour = native_cube_colour(
        sampled,
        in.material_flags,
        tint_gamma,
        in.native_ao_face,
        point_lit_terrain_light_colour(in.native_light_levels, in.world_position),
        atmosphere.fog_gamma.rgb,
        distance_fog_amount(in.world_position),
    );
    return native_colour;
}
"#;

const MARKER: &str = r#"
// Alpha reports cache admission while RGB retains all production texture and lighting bindings.
@fragment fn cache_marker(in: VertexOutput) -> @location(0) vec4<f32> {
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let colour = shade_cube(in, sample_cube_texture(in, dx, dy), dx, dy);
    return vec4(colour.rgb, in.uniform_tint_gamma.a);
}
"#;
