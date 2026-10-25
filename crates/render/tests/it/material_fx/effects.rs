//! World material effects through the host's guest library: a material reading the scene sees
//! the view's opaque colour where it is drawn, offset as it asks and clamped to the screen, and
//! its depth for soft edges; without the scene the same source reads nothing.

use super::flat::{Effects, Inputs, at, raster_effects};
use crate::gpu_snapshot::{Gpu, SNAPSHOT_SIDE};

/// A `SNAPSHOT_SIDE`-wide texture of `format` whose left half is `left` and right half
/// `right`, as RGBA8 or one 16-bit float channel.
fn halves(
    gpu: &Gpu,
    format: wgpu::TextureFormat,
    left: [u8; 8],
    right: [u8; 8],
) -> wgpu::TextureView {
    let side = SNAPSHOT_SIDE;
    let texel = format.block_copy_size(None).unwrap() as usize;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene fixture"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes: Vec<u8> = (0..side * side)
        .flat_map(|index| {
            let half = if index % side < side / 2 { left } else { right };
            half[..texel].to_vec()
        })
        .collect();
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * texel as u32),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&Default::default())
}

/// A half-float depth texel of `bits`.
fn depth(bits: u16) -> [u8; 8] {
    let [low, high] = bits.to_le_bytes();
    [low, high, 0, 0, 0, 0, 0, 0]
}

/// Half-float bits of 2 and 5.
const TWO: u16 = 0x4000;
const FIVE: u16 = 0x4500;

#[test]
fn a_scene_reading_material_sees_the_opaque_scene_offset_and_its_depth() {
    let Some(gpu) = Gpu::for_fixture("world material scene read") else {
        return;
    };
    // Red on the left half of the screen, blue on the right; the opaque surface two blocks
    // away on the left and five on the right.
    let colour = halves(
        &gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        [255, 0, 0, 255, 0, 0, 0, 0],
        [0, 0, 255, 255, 0, 0, 0, 0],
    );
    let distance = halves(&gpu, wgpu::TextureFormat::R16Float, depth(TWO), depth(FIVE));
    let scene = Effects {
        features: mod_render::world_material::Features {
            scene: true,
            ..Default::default()
        },
        scene: Some((&colour, &distance)),
        ..Effects::default()
    };
    // Refraction: the colour `param 0` pixels to the right; green is how far in front of the
    // opaque surface the fragment is, over four blocks.
    let refract = "fn shade(input: WorldMaterialInput) -> Surface {
        let behind = scene_color(vec2<f32>(param(input, 0u), 0.0));
        let gap = clamp((scene_depth(vec2<f32>(0.0)) - fragment_depth()) / 4.0, 0.0, 1.0);
        return Surface(vec3<f32>(behind.r, gap, behind.b), 1.0, vec3<f32>(0.0));
    }";
    let inputs = Inputs {
        depth: 1.0,
        ..Inputs::default()
    };
    let side = SNAPSHOT_SIDE as f32;
    let straight = raster_effects(&gpu, refract, &[], &[0.0], &inputs, &scene);
    let (left, right) = (at(&straight, 0.4, 0.5), at(&straight, 0.6, 0.5));
    assert!(left[0] > 250 && left[2] < 5, "{left:?}");
    assert!(right[2] > 250 && right[0] < 5, "{right:?}");
    // One block in front of a surface two away on the left, four in front on the right.
    assert!((60..68).contains(&left[1]), "{left:?}");
    assert!(right[1] == 255, "{right:?}");
    // Bent by a fifth of the screen, the left of centre shows the right half.
    let bent = raster_effects(&gpu, refract, &[], &[side / 5.0], &inputs, &scene);
    assert!(at(&bent, 0.4, 0.5)[2] > 250);
    // Reads past the edge clamp to the screen.
    let far = raster_effects(&gpu, refract, &[], &[side * 10.0], &inputs, &scene);
    assert!(at(&far, 0.1, 0.5)[2] > 250);
    let nan = raster_effects(&gpu, refract, &[], &[f32::NAN], &inputs, &scene);
    assert_eq!(nan, straight, "a NaN offset reads the fragment's own pixel");
    // Without the scene the same source compiles and reads no colour and far depth.
    let without = raster_effects(&gpu, refract, &[], &[0.0], &inputs, &Effects::default());
    let pixel = at(&without, 0.4, 0.5);
    assert!(pixel[0] < 5 && pixel[2] < 5 && pixel[1] == 255, "{pixel:?}");
}

/// Runs `steps` frames of the compute `source` over `cells` zeroed cells as the host does:
/// each frame dispatches one invocation per cell into the next cells and copies them over the
/// current ones, which it returns.
pub(super) fn run_compute(
    gpu: &Gpu,
    source: &str,
    cells: u32,
    params: &[f32],
    steps: u32,
) -> wgpu::Buffer {
    use mod_render::world_material::compute;
    let program = compute::compose(source).expect("the step passes the sandbox");
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world compute"),
            source: wgpu::ShaderSource::Wgsl(program.module().into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("world compute"),
            layout: None,
            module: &module,
            entry_point: Some(compute::ENTRY),
            compilation_options: Default::default(),
            cache: None,
        });
    let bytes = u64::from(cells) * 16;
    let buffer = |label| {
        gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    };
    let (current, next) = (buffer("cells"), buffer("next cells"));
    let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compute frame"),
        size: compute::FRAME_UNIFORM_BYTES as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("world compute"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: current.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: next.as_entire_binding(),
            },
        ],
    });
    for step in 0..steps {
        let words = compute::frame_uniform(step as f32 / 4096.0, cells, params);
        gpu.queue
            .write_buffer(&uniform, 0, bytemuck::cast_slice(&words));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(cells.div_ceil(compute::WORKGROUP_SIZE), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&next, 0, &current, 0, bytes);
        gpu.queue.submit([encoder.finish()]);
    }
    current
}

#[test]
fn compute_cells_step_from_the_previous_frame_and_materials_read_them() {
    let Some(gpu) = Gpu::for_fixture("world material compute cells") else {
        return;
    };
    // Each cell counts its frames and remembers its index; the last frame's phase sits in z.
    let counter = "fn step(input: WorldCellInput) -> vec4<f32> {
        let previous = cell(input.index);
        return vec4<f32>(previous.x + compute_param(0u), f32(input.index), input.phase * 4096.0, 1.0);
    }";
    let cells = run_compute(&gpu, counter, 100, &[0.25], 3);
    // A material colours each quarter of the screen by its cell: red is the count, green the
    // index over the cell count.
    let reader = "fn shade(input: WorldMaterialInput) -> Surface {
        let index = u32(input.uv.x * f32(cell_count()));
        let c = cell(index);
        return Surface(vec3<f32>(c.x, c.y / f32(cell_count()), c.z / 2.0), 1.0, vec3<f32>(0.0));
    }";
    let effects = Effects {
        features: mod_render::world_material::Features {
            cells: true,
            ..Default::default()
        },
        cells: Some(&cells),
        ..Effects::default()
    };
    let frame = raster_effects(&gpu, reader, &[], &[], &Inputs::default(), &effects);
    let left = at(&frame, 0.05, 0.5);
    let right = at(&frame, 0.95, 0.5);
    // Three steps of a quarter, from the previous frame each time; the last phase was 2.
    assert!(
        (189..=193).contains(&left[0]) && left[0] == right[0],
        "{left:?} {right:?}"
    );
    assert!(left[1] < 20 && right[1] > 230, "{left:?} {right:?}");
    assert!(left[2] > 250, "{left:?}");
    // Without cells the same material reads zero.
    let none = raster_effects(
        &gpu,
        reader,
        &[],
        &[],
        &Inputs::default(),
        &Effects::default(),
    );
    assert_eq!(&at(&none, 0.5, 0.5)[..3], &[0, 0, 0]);
}

/// Rasters the host's particle stage for `particle` with `count` particles around each of
/// `origins`, seen straight down the z axis with one block across a quarter of the frame.
pub(super) fn particles(
    gpu: &Gpu,
    particle: &str,
    count: u32,
    radius: f32,
    origins: &[[f32; 4]],
) -> Vec<u8> {
    use crate::gpu_snapshot::{Draw, RasterState};
    let features = mod_render::world_material::Features {
        particles: true,
        ..Default::default()
    };
    let material = mod_render::world_material::compose_with(
        &format!("fn shade(input: WorldMaterialInput) -> Surface {{ return surface(input); }}\n{particle}"),
        0,
        features,
    )
    .expect("the particle function passes the sandbox");
    let library = material
        .library(1)
        .replace("@group(1) @binding(0)", "@group(0) @binding(10)");
    let source = format!(
        "{library}\n{}",
        include_str!("../../../src/world_material/particles.wgsl")
    );
    let frame = gpu.buffer(
        &mod_render::world_material::frame_uniform_with_effects(
            0.0,
            1,
            [[0.0, radius, count as f32, 0.0], [0.0; 4]],
            &[],
        ),
        wgpu::BufferUsages::UNIFORM,
    );
    // Orthographic: x and y in blocks over two, depth in the middle of reverse Z.
    let scale = 0.5;
    let view = gpu.buffer(
        &[
            scale, 0.0, 0.0, 0.0, 0.0, scale, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 1.0,
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0,
        ],
        wgpu::BufferUsages::UNIFORM,
    );
    let flat: Vec<f32> = origins.iter().flatten().copied().collect();
    let targets = gpu.buffer(&flat, wgpu::BufferUsages::STORAGE);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: targets.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 10,
            resource: frame.as_entire_binding(),
        },
    ];
    gpu.render_with_state(
        &source,
        "particle_vertex",
        &[Draw {
            fragment: "particle_fragment",
            vertices: 0..4,
            bindings: &bindings,
            blend: None,
            write_depth: false,
        }],
        RasterState {
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            instance_ranges: std::iter::once(0..count * origins.len() as u32).collect(),
            ..RasterState::default()
        },
    )
}

#[test]
fn particles_square_around_each_target_within_their_radius() {
    let Some(gpu) = Gpu::for_fixture("world material particles") else {
        return;
    };
    // Two red squares of 16 texels, one block left and one block right of each target; the
    // second asks for ten blocks and stays within the radius.
    let pair = "fn particle(input: WorldParticleInput) -> WorldParticle {
        let side = select(-1.0, 10.0, input.index == 1u);
        return WorldParticle(vec3<f32>(side, 0.0, 0.0), 4.0, vec3<f32>(1.0, 0.0, 0.0), 1.0);
    }";
    let frame = particles(&gpu, pair, 2, 1.0, &[[0.0, 0.0, 0.0, 1.0]]);
    let red = |u: f32, v: f32| {
        let pixel = at(&frame, u, v);
        pixel[0] > 250 && pixel[1] < 5
    };
    // One block at a quarter of the frame per block: x = -1 and x = +1.
    assert!(red(0.5 - 0.25, 0.5) && red(0.5 + 0.25, 0.5));
    assert!(!red(0.5, 0.5), "nothing at the target itself");
    assert!(
        !red(0.95, 0.5),
        "the far particle is held within the radius"
    );
    // Each target gets its own particles.
    let two = particles(
        &gpu,
        pair,
        2,
        1.0,
        &[[0.0, 0.0, 0.0, 1.0], [0.0, 1.5, 0.0, 1.0]],
    );
    let pixel = at(&two, 0.25, 0.5 - 0.375);
    assert!(pixel[0] > 250, "{pixel:?}");
    // A particle with no size or coverage is not drawn.
    let hidden = "fn particle(input: WorldParticleInput) -> WorldParticle {
        return WorldParticle(vec3<f32>(0.0), 0.0, vec3<f32>(1.0), 1.0);
    }";
    let none = particles(&gpu, hidden, 4, 1.0, &[[0.0, 0.0, 0.0, 1.0]]);
    assert!(none.chunks(4).all(|pixel| pixel[0] == none[0]));
}
