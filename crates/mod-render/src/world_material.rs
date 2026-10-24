//! Programmable world materials: a guest `shade` function the host composes into its entity
//! and terrain pipelines. The guest computes a surface from host-provided inputs and its
//! declared textures and parameters; the host owns depth, blend, pass and visibility, and
//! applies Bedrock lighting and fog to the surface it returns.

use mod_api::{
    MAX_MATERIAL_SHADER_BYTES, MAX_MATERIAL_SHADER_EXPRESSIONS, MAX_SHADER_TEXTURE_SAMPLES,
};
use std::sync::Arc;

/// Input and surface types plus the host's bounding of guest output.
const IO: &str = include_str!("world_material/io.wgsl");
/// Entry point of the validation module; the host's own pipelines call
/// `world_material_resolve` from their fragment stages instead.
const VALIDATION_ENTRY: &str = "world_material_validate";
/// Bind group the validation module places the declared textures in; hosts choose their own.
const VALIDATION_GROUP: u32 = 0;
/// Name of the sampler binding that follows the texture bindings.
pub const SAMPLER: &str = "world_material_sampler";

/// One admitted world material, immutable and shared until every renderer reference retires.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldMaterial {
    /// The validated guest source.
    pub source: Arc<str>,
    /// Declared texture slots `texture(index, uv)` reads, in order.
    pub textures: usize,
}

/// Bytes of the `WorldMaterialFrame` uniform at binding 0: the phase row, then four rows of
/// parameters.
pub const FRAME_UNIFORM_BYTES: usize = 5 * 16;

/// The uniform a host binds at binding 0: the animation phase in `[0, 1)`, then the
/// declaration's parameters, missing ones zero.
pub fn frame_uniform(phase: f32, params: &[f32]) -> [f32; FRAME_UNIFORM_BYTES / 4] {
    let mut words = [0.0; FRAME_UNIFORM_BYTES / 4];
    words[0] = if phase.is_finite() { phase } else { 0.0 };
    for (word, value) in words[4..].iter_mut().zip(params) {
        *word = if value.is_finite() { *value } else { 0.0 };
    }
    words
}

impl WorldMaterial {
    /// The WGSL a host appends to its fragment module: the guest source, its input and
    /// surface types, `param`, `texture` and `world_material_resolve`, over the frame uniform
    /// at binding 0 of `group`, the declared textures from binding 1 and the sampler after
    /// them. `world_material_phase()` reads the phase for the host's input.
    pub fn library(&self, group: u32) -> String {
        library(group, self.textures, &self.source)
    }
}

/// The name of declared texture slot `index`'s binding.
pub fn texture_binding(index: usize) -> String {
    format!("world_material_texture_{index}")
}

/// Name of the frame uniform binding.
pub const FRAME: &str = "world_material_frame";

fn library(group: u32, textures: usize, source: &str) -> String {
    let mut out = format!(
        "struct WorldMaterialFrame {{ phase: vec4<f32>, params: array<vec4<f32>, 4> }}
@group({group}) @binding(0) var<uniform> {FRAME}: WorldMaterialFrame;
fn world_material_phase() -> f32 {{ return {FRAME}.phase.x; }}
fn param(input: WorldMaterialInput, index: u32) -> f32 {{
    let clamped = min(index, 15u);
    return {FRAME}.params[clamped / 4u][clamped % 4u];
}}
"
    );
    for index in 0..textures {
        out.push_str(&format!(
            "@group({group}) @binding({}) var {}: texture_2d<f32>;\n",
            index + 1,
            texture_binding(index)
        ));
    }
    if textures != 0 {
        out.push_str(&format!(
            "@group({group}) @binding({}) var {SAMPLER}: sampler;\n",
            textures + 1
        ));
    }
    out.push_str("fn texture(index: u32, uv: vec2<f32>) -> vec4<f32> {\n    switch index {\n");
    for index in 0..textures {
        out.push_str(&format!(
            "        case {index}u: {{ return textureSampleLevel({}, {SAMPLER}, uv, 0.0); }}\n",
            texture_binding(index)
        ));
    }
    out.push_str("        default: { return vec4<f32>(0.0); }\n    }\n}\n");
    out.push_str(&IO.replace(
        "MAX_EMISSION",
        &format!("{:.1}", mod_api::MAX_MATERIAL_EMISSION),
    ));
    out.push_str("// guest\n");
    out.push_str(source);
    out.push('\n');
    out
}

/// Validates a guest material against the sandbox: the `shade` signature, no resources,
/// module variables, overrides, entry points, loops, recursion or storage writes, and bounded
/// expanded expressions and texture reads. `textures` is the declaration's count, which the
/// caller has bounded.
pub fn compose(source: &str, textures: usize) -> Result<WorldMaterial, String> {
    if source.len() > MAX_MATERIAL_SHADER_BYTES {
        return Err("world material exceeds byte limit".into());
    }
    crate::shader::check_statement_tokens(source)?;
    let material = WorldMaterial {
        source: source.into(),
        textures,
    };
    let checked = material.clone();
    std::thread::Builder::new()
        .name("world material validation".into())
        .stack_size(crate::shader::VALIDATION_STACK_BYTES)
        .spawn(move || validate(&checked))
        .map_err(|_| "world material validation unavailable".to_owned())?
        .join()
        .map_err(|_| "world material validation failed".to_owned())??;
    Ok(material)
}

/// The validation module: the library over the declared bindings and one fragment entry that
/// resolves a host-shaped input.
fn module(material: &WorldMaterial) -> String {
    format!(
        "{}
@fragment fn {VALIDATION_ENTRY}(@location(0) uv: vec2<f32>, @location(1) world: vec3<f32>,
    @location(2) base: vec4<f32>, @location(3) light: vec4<f32>, @location(4) fog: vec4<f32>,
    @location(5) overlay: vec4<f32>, @location(6) phase: f32) -> @location(0) vec4<f32> {{
    let surface = world_material_resolve(WorldMaterialInput(uv, world, base, light, fog, overlay, world_material_phase() + phase));
    return vec4<f32>(surface.color + surface.emission, surface.coverage);
}}
",
        material.library(VALIDATION_GROUP)
    )
}

fn validate(material: &WorldMaterial) -> Result<(), String> {
    let composed = module(material);
    let module = naga::front::wgsl::parse_str(&composed)
        .map_err(|_| "world material WGSL or shade signature invalid".to_owned())?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|_| "world material WGSL invalid".to_owned())?;
    let mut expected: Vec<String> = (0..material.textures).map(texture_binding).collect();
    expected.push(FRAME.into());
    if material.textures != 0 {
        expected.push(SAMPLER.into());
    }
    expected.sort_unstable();
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    crate::shader::check_sandbox(
        &module,
        &expected,
        &[VALIDATION_ENTRY],
        VALIDATION_ENTRY,
        MAX_SHADER_TEXTURE_SAMPLES,
        MAX_MATERIAL_SHADER_EXPRESSIONS,
    )
}

#[cfg(test)]
mod tests;
