//! Render capability limits shared by native and guest code.

/// Render capability budgets, per instance or per committed callback.
pub const MAX_RENDER_PASSES: usize = 8;
/// Render and retained drawing imports allowed in one callback, including denied calls.
pub const MAX_RENDER_CALLS: usize = 32;
/// Shader validations allowed in one frame callback; `init` may compile every pass.
pub const MAX_PASS_COMPILES_PER_FRAME: u32 = 1;
pub const MAX_PASS_NAME_BYTES: usize = 32;
pub const MAX_PASS_PARAMS: usize = 16;
pub const MAX_SHADER_BYTES: usize = 16 * 1024;
/// Worst-case texture reads and IR expressions one fragment may execute, helpers included.
pub const MAX_SHADER_TEXTURE_SAMPLES: u32 = 32;
pub const MAX_SHADER_EXPRESSIONS: u32 = 4096;
/// Bounds expression nesting, which is otherwise one level per operator in a statement.
pub const MAX_STATEMENT_TOKENS: usize = 512;
/// Largest value any shader type may hold, which bounds per-pixel private memory.
pub const MAX_SHADER_TYPE_BYTES: u32 = 1024;
pub const MAX_RENDER_DECALS: usize = 64;
pub const MAX_RENDER_RIBBONS: usize = 32;
pub const MAX_RIBBON_POINTS: usize = 64;
pub const MAX_RENDER_BEAMS: usize = 16;
pub const MAX_RENDER_BILLBOARDS: usize = 512;
/// Largest decal radius, ribbon or beam width, or billboard side, in blocks.
pub const MAX_PRIMITIVE_EXTENT_BLOCKS: f32 = 64.0;
/// Primitives farther than this from the origin of either axis are rejected.
pub const MAX_PRIMITIVE_COORDINATE: f32 = 30_000_000.0;
