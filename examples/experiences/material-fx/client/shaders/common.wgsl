// Shared helpers of the showcase materials, prepended to each one: block-local coordinates,
// the 16-texel grid every effect snaps to, hashes, a limited palette and ray-box exits.
const TAU: f32 = 6.2831853;
// Texels per block edge; every effect snaps to this grid.
const TEXELS: f32 = 16.0;

// Seconds on the session clock, which follows the wall clock modulo 4096 s, so the server can
// time events with it.
fn seconds(input: WorldMaterialInput) -> f32 {
    return input.phase * 4096.0;
}

// Seconds since `start` on that clock, across its wrap.
fn since(input: WorldMaterialInput, start: f32) -> f32 {
    return fract((seconds(input) - start) / 4096.0 + 1.0) * 4096.0;
}

// The block the fragment's face belongs to.
fn cell_of(input: WorldMaterialInput) -> vec3<f32> {
    return floor(input.world_position - input.normal * 0.001);
}

// The fragment's position inside its block, in [0, 1].
fn local_of(input: WorldMaterialInput) -> vec3<f32> {
    return clamp(input.world_position - cell_of(input), vec3<f32>(0.0), vec3<f32>(1.0));
}

// The centre of the texel of the 16-texel grid that `v` (in blocks) falls in.
fn snap3(v: vec3<f32>) -> vec3<f32> {
    return (floor(v * TEXELS) + 0.5) / TEXELS;
}

fn snap2(v: vec2<f32>) -> vec2<f32> {
    return (floor(v * TEXELS) + 0.5) / TEXELS;
}

// The face-plane coordinates of block-local `local` for a face with `normal`: across, then up
// (or along Z for horizontal faces).
fn face_uv(local: vec3<f32>, normal: vec3<f32>) -> vec2<f32> {
    let n = abs(normal);
    if n.y > 0.5 {
        return local.xz;
    }
    if n.x > 0.5 {
        return vec2<f32>(local.z, local.y);
    }
    return local.xy;
}

// A hash in [0, 1) of a lattice point.
fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q = q + dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn hash2(p: vec2<f32>) -> f32 {
    return hash3(vec3<f32>(p, 17.0));
}

// `c` reduced to `levels` steps per channel, the showcase's limited palette.
fn posterize(c: vec3<f32>, levels: f32) -> vec3<f32> {
    return floor(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * levels + 0.5) / levels;
}

// Distance along `dir` from `origin` to where it leaves the box [lo, hi], which contains it.
fn box_exit(origin: vec3<f32>, dir: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> f32 {
    let inverse = 1.0 / select(dir, vec3<f32>(1e-6), abs(dir) < vec3<f32>(1e-6));
    let far = max((lo - origin) * inverse, (hi - origin) * inverse);
    return max(min(min(far.x, far.y), far.z), 0.0);
}

// Distance along `dir` from `origin` to where it enters the box [lo, hi], or -1 when it misses.
fn box_entry(origin: vec3<f32>, dir: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> f32 {
    let inverse = 1.0 / select(dir, vec3<f32>(1e-6), abs(dir) < vec3<f32>(1e-6));
    let a = (lo - origin) * inverse;
    let b = (hi - origin) * inverse;
    let near = max(max(min(a.x, b.x), min(a.y, b.y)), min(a.z, b.z));
    let far = min(min(max(a.x, b.x), max(a.y, b.y)), max(a.z, b.z));
    return select(-1.0, max(near, 0.0), far >= max(near, 0.0));
}

// The outward normal of the face of the unit cell centred at `centre` that `p` lies on.
fn cell_normal(p: vec3<f32>, centre: vec3<f32>) -> vec3<f32> {
    let q = p - centre;
    let a = abs(q);
    if a.x >= a.y && a.x >= a.z {
        return vec3<f32>(sign(q.x), 0.0, 0.0);
    }
    if a.y >= a.z {
        return vec3<f32>(0.0, sign(q.y), 0.0);
    }
    return vec3<f32>(0.0, 0.0, sign(q.z));
}

// `v` (in blocks) rounded to whole texels, so moved geometry keeps to the pixel grid.
fn to_texels(v: vec3<f32>) -> vec3<f32> {
    return round(v * TEXELS) / TEXELS;
}

// Seconds on the session clock in a vertex or particle stage.
fn seconds_at(phase: f32) -> f32 {
    return phase * 4096.0;
}

// Seconds since `start` on the session clock in a vertex or particle stage.
fn since_at(phase: f32, start: f32) -> f32 {
    return fract((seconds_at(phase) - start) / 4096.0 + 1.0) * 4096.0;
}

// The camera's right and up directions for a view direction `d`, the camera never rolling.
fn camera_axes(d: vec3<f32>) -> mat2x3<f32> {
    let right = normalize(cross(d, vec3<f32>(0.0, 1.0, 0.0)) + vec3<f32>(1e-5, 0.0, 0.0));
    return mat2x3<f32>(right, cross(right, d));
}

// The screen offset, in pixels, that turning the view ray from `d` to `bent` moves its hit by,
// at `pixels` pixels per radian.
fn screen_shift(d: vec3<f32>, bent: vec3<f32>, pixels: f32) -> vec2<f32> {
    let axes = camera_axes(d);
    let turn = bent - d;
    return vec2<f32>(dot(turn, axes[0]), -dot(turn, axes[1])) * pixels;
}
