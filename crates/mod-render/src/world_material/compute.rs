//! World material compute: a guest `step` function the host runs once per cell per frame over
//! cells it owns for the bundle. Each invocation returns its own cell's next value from the
//! previous frame's cells; the guest writes nothing itself, reads no host resource, and
//! nothing reads the cells back to it.

use mod_api::{MAX_MATERIAL_SHADER_BYTES, MAX_MATERIAL_SHADER_EXPRESSIONS};
use std::sync::Arc;

/// Invocations per workgroup of the host's compute entry.
pub const WORKGROUP_SIZE: u32 = 64;
/// The host's compute entry point.
pub const ENTRY: &str = "world_compute_main";
/// Bytes of the compute frame uniform: the phase row (phase, cell count), then four rows of
/// parameters.
pub const FRAME_UNIFORM_BYTES: usize = 5 * 16;
/// Name of the uniform binding.
const FRAME: &str = "world_compute_frame";
/// Name of the previous frame's cells, which `cell` reads.
const PREVIOUS: &str = "world_cells_previous";
/// Name of the cells the entry writes; only host code names it.
const NEXT: &str = "world_cells_next";

/// One admitted compute program, immutable and shared until every renderer reference retires.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldCompute {
    /// The validated guest source.
    pub source: Arc<str>,
}

/// The uniform a host binds at binding 0: the phase and the cell count, then the parameters,
/// missing or non-finite ones zero.
pub fn frame_uniform(phase: f32, cells: u32, params: &[f32]) -> [f32; FRAME_UNIFORM_BYTES / 4] {
    let mut words = [0.0; FRAME_UNIFORM_BYTES / 4];
    words[0] = if phase.is_finite() { phase } else { 0.0 };
    words[1] = cells as f32;
    for (word, value) in words[4..].iter_mut().zip(params) {
        *word = if value.is_finite() { *value } else { 0.0 };
    }
    words
}

/// Shared declarations: the input type, the uniform at binding 0 and the previous cells at
/// binding 1 of group 0, `compute_param`, `cell` and `cell_count`.
fn prelude() -> String {
    format!(
        "struct WorldCellInput {{
    index: u32,
    count: u32,
    phase: f32,
}}
struct WorldComputeFrame {{ phase: vec4<f32>, params: array<vec4<f32>, 4> }}
@group(0) @binding(0) var<uniform> {FRAME}: WorldComputeFrame;
@group(0) @binding(1) var<storage, read> {PREVIOUS}: array<vec4<f32>>;
fn compute_param(index: u32) -> f32 {{
    let clamped = min(index, 15u);
    return {FRAME}.params[clamped / 4u][clamped % 4u];
}}
fn cell_count() -> u32 {{ return arrayLength(&{PREVIOUS}); }}
// Cell `index` as the previous frame left it, clamped to the last.
fn cell(index: u32) -> vec4<f32> {{
    let count = arrayLength(&{PREVIOUS});
    if count == 0u {{ return vec4<f32>(0.0); }}
    return {PREVIOUS}[min(index, count - 1u)];
}}
// The guest's next value, with every number that is not finite zero.
fn world_compute_step(input: WorldCellInput) -> vec4<f32> {{
    let next = step(input);
    return select(vec4<f32>(0.0), next, (next == next) & (abs(next) <= vec4<f32>(3.0e38)));
}}
"
    )
}

impl WorldCompute {
    /// The program's content identity: the first 128 bits of the SHA-256 of its validated
    /// source, which is all its module depends on.
    pub fn program(&self) -> u128 {
        use sha2::Digest;
        let bytes: [u8; 32] = sha2::Sha256::digest(self.source.as_bytes()).into();
        let mut prefix = [0; 16];
        prefix.copy_from_slice(&bytes[..16]);
        u128::from_le_bytes(prefix)
    }

    /// The host's module: the prelude, the next cells at binding 2 and the entry that sets
    /// each cell below the count to `step`'s result, then the guest source.
    pub fn module(&self) -> String {
        format!(
            "{}
@group(0) @binding(2) var<storage, read_write> {NEXT}: array<vec4<f32>>;
@compute @workgroup_size({WORKGROUP_SIZE}) fn {ENTRY}(@builtin(global_invocation_id) id: vec3<u32>) {{
    let count = arrayLength(&{NEXT});
    if id.x >= count {{ return; }}
    {NEXT}[id.x] = world_compute_step(WorldCellInput(id.x, count, {FRAME}.phase.x));
}}
// guest
{}
",
            prelude(),
            self.source
        )
    }
}

/// The validation module: the prelude and an entry that runs `step` without any writable
/// binding, so a guest naming the host's next cells does not compile.
fn validation_module(source: &str) -> String {
    format!(
        "{}
@compute @workgroup_size({WORKGROUP_SIZE}) fn {ENTRY}(@builtin(global_invocation_id) id: vec3<u32>) {{
    let next = world_compute_step(WorldCellInput(id.x, cell_count(), {FRAME}.phase.x));
    if next.x > 1.0e30 {{ return; }}
}}
// guest
{source}
",
        prelude()
    )
}

/// Validates a guest compute program against the sandbox: the `step` signature, no resources,
/// module variables, overrides, entry points, loops, recursion or storage writes, and a
/// bounded expanded expression count per cell.
pub fn compose(source: &str) -> Result<WorldCompute, String> {
    if source.len() > MAX_MATERIAL_SHADER_BYTES {
        return Err("world compute exceeds byte limit".into());
    }
    crate::shader::check_statement_tokens(source)?;
    let module = validation_module(source);
    std::thread::Builder::new()
        .name("world compute validation".into())
        .stack_size(crate::shader::VALIDATION_STACK_BYTES)
        .spawn(move || validate(&module))
        .map_err(|_| "world compute validation unavailable".to_owned())?
        .join()
        .map_err(|_| "world compute validation failed".to_owned())??;
    Ok(WorldCompute {
        source: source.into(),
    })
}

fn validate(composed: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(composed)
        .map_err(|_| "world compute WGSL or step signature invalid".to_owned())?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|_| "world compute WGSL invalid".to_owned())?;
    let mut expected = [FRAME, PREVIOUS];
    expected.sort_unstable();
    crate::shader::check_sandbox(
        &module,
        &expected,
        &[ENTRY],
        ENTRY,
        0,
        MAX_MATERIAL_SHADER_EXPRESSIONS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIFE: &str = "fn step(input: WorldCellInput) -> vec4<f32> {
        let left = cell(input.index + cell_count() - 1u).x;
        let right = cell(input.index + 1u).x;
        let alive = select(0.0, 1.0, (left + right) * compute_param(0u) > 0.5);
        return vec4<f32>(alive, input.phase, 0.0, 1.0);
    }";

    #[test]
    fn a_step_reads_the_previous_cells_and_returns_its_own_next_value() {
        let compute = compose(LIFE).unwrap();
        let module = compute.module();
        assert!(module.contains("var<storage, read_write> world_cells_next"));
        // The production module the host builds is valid WGSL.
        let parsed = naga::front::wgsl::parse_str(&module).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&parsed)
        .unwrap();
        let frame = frame_uniform(0.5, 64, &[1.0, f32::INFINITY]);
        assert_eq!(
            (frame[0], frame[1], frame[4], frame[5]),
            (0.5, 64.0, 1.0, 0.0)
        );
    }

    #[test]
    fn hostile_steps_are_rejected() {
        for source in [
            // Writing cells other than its own, or any host resource.
            "fn step(input: WorldCellInput) -> vec4<f32> { world_cells_next[0] = vec4<f32>(1.0); return vec4<f32>(0.0); }".to_owned(),
            format!("@group(0) @binding(3) var<storage, read_write> stolen: array<f32>; {LIFE}"),
            format!("var<private> state: f32; {LIFE}"),
            "fn step(input: WorldCellInput) -> vec4<f32> { loop {} return vec4<f32>(0.0); }".into(),
            "fn step(input: WorldCellInput) -> vec4<f32> { return step(input); }".into(),
            "fn step(input: WorldCellInput) -> f32 { return 0.0; }".into(),
            format!("@compute @workgroup_size(1) fn guest() {{}} {LIFE}"),
        ] {
            assert!(compose(&source).is_err(), "accepted {source}");
        }
        let mut source = "fn f0(x: f32) -> f32 { return x*x; }".to_owned();
        for index in 1..14 {
            let previous = index - 1;
            source.push_str(&format!(
                "fn f{index}(x: f32) -> f32 {{ return f{previous}(x)+f{previous}(x); }}"
            ));
        }
        source.push_str(
            "fn step(input: WorldCellInput) -> vec4<f32> { return vec4<f32>(f13(input.phase)); }",
        );
        assert!(compose(&source).unwrap_err().contains("expressions"));
    }
}
