//! What a world material's module provides beyond `shade`: the guest functions each effect
//! calls, their input types, the host's bounding of what they return, and the resources the
//! scene and cells effects read. The host enables each through its own permission.

use super::FRAME;

/// The guest functions and host resources a world material's module has beyond `shade`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Features {
    /// `fn displace(input: WorldVertexInput) -> vec3<f32>` moves each vertex, clamped by the
    /// host to the frame's displacement limit.
    pub displace: bool,
    /// `scene_color`, `scene_depth` and `fragment_depth` read the view's opaque colour and
    /// depth already drawn.
    pub scene: bool,
    /// `cell` and `cell_count` read the bundle's compute cells.
    pub cells: bool,
    /// `fn outline(input: WorldMaterialInput) -> vec3<f32>` colours the outline pass.
    pub outline: bool,
    /// `fn particle(input: WorldParticleInput) -> WorldParticle` places and colours each
    /// particle of the particle pass.
    pub particles: bool,
}

/// Where a material's feature resources sit in its bind group, after the frame uniform, the
/// declared textures and their sampler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialBindings {
    /// The view's opaque colour, a `texture_2d<f32>` read with `textureLoad`.
    pub scene_color: Option<u32>,
    /// The view's opaque depth as linear view depth in blocks, an unfilterable
    /// `texture_2d<f32>` read with `textureLoad`.
    pub scene_depth: Option<u32>,
    /// The bundle's compute cells, a read-only storage `array<vec4<f32>>`.
    pub cells: Option<u32>,
}

impl MaterialBindings {
    /// The bindings of a material with `textures` declared textures and `features`.
    pub fn of(textures: usize, features: Features) -> Self {
        let mut next = textures as u32 + 1 + u32::from(textures != 0);
        let mut take = |enabled: bool| {
            enabled.then(|| {
                next += 1;
                next - 1
            })
        };
        let scene_color = take(features.scene);
        let scene_depth = take(features.scene);
        let cells = take(features.cells);
        Self {
            scene_color,
            scene_depth,
            cells,
        }
    }
}

/// Name of the scene colour binding.
pub const SCENE_COLOR: &str = "world_material_scene_color";
/// Name of the scene depth binding.
pub const SCENE_DEPTH: &str = "world_material_scene_depth";
/// Name of the cells binding.
pub const CELLS: &str = "world_material_cells";
/// The private fragment position the scene functions read, which the host sets.
const FRAGMENT: &str = "world_material_fragment";

/// Module-scope names the library declares for `features`, besides the frame, textures and
/// sampler.
pub(super) fn globals(features: Features) -> impl Iterator<Item = &'static str> {
    let scene: &[&str] = if features.scene {
        &[SCENE_COLOR, SCENE_DEPTH, FRAGMENT]
    } else {
        &[]
    };
    let cells: &[&str] = if features.cells { &[CELLS] } else { &[] };
    scene.iter().chain(cells).copied()
}

/// Input and output types of every effect function, declared whatever the features, so a
/// material whose effect is withdrawn still compiles.
const TYPES: &str = "struct WorldVertexInput {
    world_position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    phase: f32,
}
struct WorldParticleInput {
    index: u32,
    count: u32,
    phase: f32,
    // The bound target's position in the world.
    origin: vec3<f32>,
}
struct WorldParticle {
    // Where the particle is, in blocks from the target; the host keeps it within the radius.
    offset: vec3<f32>,
    // Its side in texels, at most four; zero hides it.
    size: f32,
    // Its colour, drawn as emission, and its coverage.
    color: vec3<f32>,
    coverage: f32,
}
";

/// What the scene functions return without the scene: no colour, and depth so far that every
/// fragment counts as clear of the scene.
const NO_SCENE: &str = "fn world_material_set_fragment(pixel: vec2<f32>, depth: f32) {}
fn scene_color(offset: vec2<f32>) -> vec3<f32> { return vec3<f32>(0.0); }
fn scene_depth(offset: vec2<f32>) -> f32 { return 1.0e9; }
fn fragment_depth() -> f32 { return 0.0; }
";

/// What the cell functions return without cells: none.
const NO_CELLS: &str = "fn cell_count() -> u32 { return 0u; }
fn cell(index: u32) -> vec4<f32> { return vec4<f32>(0.0); }
";

/// The library part for `features`, after the base library of a material with `textures`
/// textures in bind group `group`. Functions of a feature that is off still exist and return
/// neutral values, so withdrawing an effect never stops a material compiling.
pub(super) fn library(group: u32, textures: usize, features: Features) -> String {
    let bindings = MaterialBindings::of(textures, features);
    let mut out = String::from(TYPES);
    if !features.scene {
        out.push_str(NO_SCENE);
    }
    if !features.cells {
        out.push_str(NO_CELLS);
    }
    if let (Some(color), Some(depth)) = (bindings.scene_color, bindings.scene_depth) {
        out.push_str(&format!(
            "@group({group}) @binding({color}) var {SCENE_COLOR}: texture_2d<f32>;
@group({group}) @binding({depth}) var {SCENE_DEPTH}: texture_2d<f32>;
var<private> {FRAGMENT}: vec4<f32>;
// The host sets the fragment's pixel and linear view depth before `shade` runs.
fn world_material_set_fragment(pixel: vec2<f32>, depth: f32) {{
    {FRAGMENT} = vec4<f32>(pixel, depth, 0.0);
}}
fn world_material_scene_pixel(offset: vec2<f32>) -> vec2<i32> {{
    let size = vec2<i32>(textureDimensions({SCENE_COLOR}));
    let wanted = {FRAGMENT}.xy + select(vec2<f32>(0.0), offset, offset == offset);
    let pixel = vec2<i32>(clamp(wanted, vec2<f32>(0.0), vec2<f32>(size - vec2<i32>(1))));
    return pixel;
}}
// The opaque colour already drawn `offset` pixels from this fragment, clamped to the screen.
fn scene_color(offset: vec2<f32>) -> vec3<f32> {{
    return textureLoad({SCENE_COLOR}, world_material_scene_pixel(offset), 0).rgb;
}}
// The linear view depth, in blocks, of the opaque surface `offset` pixels from this fragment.
fn scene_depth(offset: vec2<f32>) -> f32 {{
    return textureLoad({SCENE_DEPTH}, world_material_scene_pixel(offset), 0).r;
}}
// This fragment's linear view depth in blocks.
fn fragment_depth() -> f32 {{ return {FRAGMENT}.z; }}
"
        ));
    }
    if let Some(cells) = bindings.cells {
        out.push_str(&format!(
            "@group({group}) @binding({cells}) var<storage, read> {CELLS}: array<vec4<f32>>;
// How many cells the bundle's compute declaration holds.
fn cell_count() -> u32 {{ return arrayLength(&{CELLS}); }}
// Cell `index`, clamped to the last; zero when there are none.
fn cell(index: u32) -> vec4<f32> {{
    let count = arrayLength(&{CELLS});
    if count == 0u {{ return vec4<f32>(0.0); }}
    return {CELLS}[min(index, count - 1u)];
}}
"
        ));
    }
    if features.displace {
        out.push_str(&format!(
            "// One axis of a displacement, clamped to the limit; NaN is zero.
fn world_material_clamp_axis(value: f32, limit: f32) -> f32 {{
    if value >= -limit && value <= limit {{ return value; }}
    if value > limit {{ return limit; }}
    if value < -limit {{ return -limit; }}
    return 0.0;
}}
// The guest's displacement, clamped on each axis to the declared limit in blocks.
fn world_material_displace(input: WorldVertexInput) -> vec3<f32> {{
    let offset = displace(input);
    let limit = {FRAME}.effects.x;
    return vec3<f32>(
        world_material_clamp_axis(offset.x, limit),
        world_material_clamp_axis(offset.y, limit),
        world_material_clamp_axis(offset.z, limit),
    );
}}
"
        ));
    }
    if features.outline {
        out.push_str(
            "// The guest's outline colour, bounded as emission.
fn world_material_outline(input: WorldMaterialInput) -> vec3<f32> {
    let colour = outline(input);
    return vec3<f32>(world_material_bounded(colour.x, MAX_EMISSION), world_material_bounded(colour.y, MAX_EMISSION), world_material_bounded(colour.z, MAX_EMISSION));
}
",
        );
    }
    if features.particles {
        out.push_str(&format!(
            "// The guest's particle, its offset within the radius, its size within four texels and its
// colour and coverage bounded.
fn world_material_particle(input: WorldParticleInput) -> WorldParticle {{
    let particle = particle(input);
    let radius = {FRAME}.effects.z;
    let offset = vec3<f32>(
        world_material_clamp_particle(particle.offset.x, radius),
        world_material_clamp_particle(particle.offset.y, radius),
        world_material_clamp_particle(particle.offset.z, radius),
    );
    return WorldParticle(
        offset,
        world_material_bounded(particle.size, 4.0),
        vec3<f32>(world_material_bounded(particle.color.x, MAX_EMISSION), world_material_bounded(particle.color.y, MAX_EMISSION), world_material_bounded(particle.color.z, MAX_EMISSION)),
        world_material_bounded(particle.coverage, 1.0),
    );
}}
fn world_material_clamp_particle(value: f32, limit: f32) -> f32 {{
    if value >= -limit && value <= limit {{ return value; }}
    if value > limit {{ return limit; }}
    if value < -limit {{ return -limit; }}
    return 0.0;
}}
"
        ));
    }
    out.replace(
        "MAX_EMISSION",
        &format!("{:.1}", mod_api::MAX_MATERIAL_EMISSION),
    )
}

/// Entry point names of the validation module's further entries for `features`.
pub(super) fn entry_names(features: Features) -> impl Iterator<Item = &'static str> {
    [
        (features.displace, "world_material_validate_vertex"),
        (features.outline, "world_material_validate_outline"),
        (features.particles, "world_material_validate_particle"),
    ]
    .into_iter()
    .filter_map(|(enabled, name)| enabled.then_some(name))
}

/// The validation module's further entries for `features`: one per function the host runs,
/// so each is bounded on its own.
pub(super) fn validation_entries(features: Features) -> String {
    let mut out = String::new();
    if features.displace {
        out.push_str(
            "@vertex fn world_material_validate_vertex(@location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> @builtin(position) vec4<f32> {
    let moved = world + world_material_displace(WorldVertexInput(world, normal, uv, world_material_phase()));
    return vec4<f32>(moved, 1.0);
}
",
        );
    }
    if features.outline {
        out.push_str(
            "@fragment fn world_material_validate_outline(@location(0) uv: vec2<f32>, @location(1) world: vec3<f32>,
    @location(2) base: vec4<f32>, @location(3) light: vec4<f32>, @location(4) fog: vec4<f32>,
    @location(5) overlay: vec4<f32>, @location(6) normal: vec3<f32>,
    @location(7) view: vec3<f32>) -> @location(0) vec4<f32> {
    let colour = world_material_outline(WorldMaterialInput(uv, world, base, light, fog, overlay, world_material_phase(), normal, view));
    return vec4<f32>(colour, 1.0);
}
",
        );
    }
    if features.particles {
        out.push_str(
            "@vertex fn world_material_validate_particle(@builtin(instance_index) index: u32,
    @location(0) origin: vec3<f32>) -> @builtin(position) vec4<f32> {
    let particle = world_material_particle(WorldParticleInput(index, 1u, world_material_phase(), origin));
    return vec4<f32>(origin + particle.offset, particle.size + particle.coverage + particle.color.x);
}
",
        );
    }
    out
}
