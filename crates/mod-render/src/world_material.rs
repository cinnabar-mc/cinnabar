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
    /// The guest functions and host resources its module has beyond `shade`.
    pub features: Features,
}

pub use effects::{Features, MaterialBindings};

/// Bytes of the `WorldMaterialFrame` uniform at binding 0: the phase row, the effects row,
/// then four rows of parameters.
pub const FRAME_UNIFORM_BYTES: usize = 6 * 16;

/// The uniform a host binds at binding 0 for a material without effects:
/// [`frame_uniform_with_effects`] with a zero effects row.
pub fn frame_uniform(phase: f32, tag: u8, params: &[f32]) -> [f32; FRAME_UNIFORM_BYTES / 4] {
    frame_uniform_with_effects(phase, tag, [0.0; 4], params)
}

/// The uniform a host binds at binding 0: the animation phase in `[0, 1)` and the quad tag
/// its terrain draws keep (one more than the material's slot); the effects row (the
/// displacement limit in blocks, the outline width in blocks, the particle radius in blocks
/// and the particle count); then the declaration's parameters, missing ones zero. Values that
/// are not finite are zero.
pub fn frame_uniform_with_effects(
    phase: f32,
    tag: u8,
    effects: [f32; 4],
    params: &[f32],
) -> [f32; FRAME_UNIFORM_BYTES / 4] {
    let finite = |value: f32| if value.is_finite() { value } else { 0.0 };
    let mut words = [0.0; FRAME_UNIFORM_BYTES / 4];
    words[0] = finite(phase);
    words[1] = f32::from(tag);
    for (word, value) in words[4..8].iter_mut().zip(effects) {
        *word = finite(value);
    }
    for (word, value) in words[8..].iter_mut().zip(params) {
        *word = finite(*value);
    }
    words
}

impl WorldMaterial {
    /// The program's content identity: the first 128 bits of the SHA-256 of its texture count,
    /// effect features and validated source, which are all its composed modules depend on. A
    /// collision-resistant digest, so one guest cannot make another's program draw its module.
    pub fn program(&self) -> u128 {
        use sha2::Digest;
        let features = &self.features;
        let flags = [
            features.displace,
            features.scene,
            features.cells,
            features.outline,
            features.particles,
        ]
        .iter()
        .enumerate()
        .fold(0_u8, |flags, (bit, set)| flags | u8::from(*set) << bit);
        let mut digest = sha2::Sha256::new();
        digest.update((self.textures as u64).to_le_bytes());
        digest.update([flags]);
        digest.update(self.source.as_bytes());
        let bytes: [u8; 32] = digest.finalize().into();
        let mut prefix = [0; 16];
        prefix.copy_from_slice(&bytes[..16]);
        u128::from_le_bytes(prefix)
    }

    /// The WGSL a host appends to its module: the guest source, its input and surface types,
    /// `param`, `parameter`, `texture` and `world_material_resolve`, over the frame uniform at
    /// binding 0 of `group`, the declared textures from binding 1 and the sampler after them,
    /// then the resources of its [`Features`] at [`Self::bindings`].
    /// `world_material_phase()` reads the phase for the host's input and
    /// `world_material_tag()` the quad tag a terrain draw keeps.
    pub fn library(&self, group: u32) -> String {
        library(group, self.textures, self.features, &self.source)
    }

    /// Where its feature resources sit in its bind group.
    pub fn bindings(&self) -> MaterialBindings {
        MaterialBindings::of(self.textures, self.features)
    }
}

/// The name of declared texture slot `index`'s binding.
pub fn texture_binding(index: usize) -> String {
    format!("world_material_texture_{index}")
}

/// Name of the frame uniform binding.
pub const FRAME: &str = "world_material_frame";

fn library(group: u32, textures: usize, features: Features, source: &str) -> String {
    let mut out = format!(
        "struct WorldMaterialFrame {{ phase: vec4<f32>, effects: vec4<f32>, params: array<vec4<f32>, 4> }}
@group({group}) @binding(0) var<uniform> {FRAME}: WorldMaterialFrame;
fn world_material_phase() -> f32 {{ return {FRAME}.phase.x; }}
fn world_material_tag() -> u32 {{ return u32({FRAME}.phase.y); }}
fn parameter(index: u32) -> f32 {{
    let clamped = min(index, 15u);
    return {FRAME}.params[clamped / 4u][clamped % 4u];
}}
fn param(input: WorldMaterialInput, index: u32) -> f32 {{ return parameter(index); }}
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
    out.push_str(&effects::library(group, textures, features));
    out.push_str("// guest\n");
    out.push_str(source);
    out.push('\n');
    out
}

/// Validates a guest material without effects: [`compose_with`] with no [`Features`].
pub fn compose(source: &str, textures: usize) -> Result<WorldMaterial, String> {
    compose_with(source, textures, Features::default())
}

/// Validates a guest material against the sandbox: the `shade` signature and the signature of
/// each function its `features` call, no resources, module variables, overrides, entry
/// points, loops, recursion or storage writes, and bounded expanded expressions and texture
/// reads for each function the host runs per pixel or per vertex. `textures` is the
/// declaration's count, which the caller has bounded.
pub fn compose_with(
    source: &str,
    textures: usize,
    features: Features,
) -> Result<WorldMaterial, String> {
    if source.len() > MAX_MATERIAL_SHADER_BYTES {
        return Err("world material exceeds byte limit".into());
    }
    crate::shader::check_statement_tokens(source)?;
    let material = WorldMaterial {
        source: source.into(),
        textures,
        features,
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

/// The validation module: the library over the declared bindings, one fragment entry that
/// resolves a host-shaped input, and an entry for each further function its features call.
fn module(material: &WorldMaterial) -> String {
    let scene = if material.features.scene {
        "world_material_set_fragment(uv * 64.0, length(world));\n    "
    } else {
        ""
    };
    format!(
        "{}
@fragment fn {VALIDATION_ENTRY}(@location(0) uv: vec2<f32>, @location(1) world: vec3<f32>,
    @location(2) base: vec4<f32>, @location(3) light: vec4<f32>, @location(4) fog: vec4<f32>,
    @location(5) overlay: vec4<f32>, @location(6) phase: f32, @location(7) normal: vec3<f32>,
    @location(8) view: vec3<f32>) -> @location(0) vec4<f32> {{
    {scene}let surface = world_material_resolve(WorldMaterialInput(uv, world, base, light, fog, overlay, world_material_phase() + phase, normal, view));
    return vec4<f32>(surface.color + surface.emission, surface.coverage);
}}
{}",
        material.library(VALIDATION_GROUP),
        effects::validation_entries(material.features),
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
    expected.extend(effects::globals(material.features).map(str::to_owned));
    expected.sort_unstable();
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    let mut entries: Vec<&str> = std::iter::once(VALIDATION_ENTRY)
        .chain(effects::entry_names(material.features))
        .collect();
    entries.sort_unstable();
    for entry in &entries {
        crate::shader::check_sandbox(
            &module,
            &expected,
            &entries,
            entry,
            MAX_SHADER_TEXTURE_SAMPLES,
            MAX_MATERIAL_SHADER_EXPRESSIONS,
        )?;
    }
    Ok(())
}

pub mod compute;
mod effects;

pub mod publication;
#[cfg(test)]
mod tests;
