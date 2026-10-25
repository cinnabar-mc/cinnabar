//! A camera looking at a box of blocks: each pixel's ray is traced to the box, and the face it
//! meets hands `shade` its world position, normal and the direction back to the camera, as the
//! terrain pass would, so volumetric effects can be checked from any viewpoint.

use super::flat::raster_with;
use crate::gpu_snapshot::Gpu;

/// A pinhole camera at `eye` looking at `target` with a vertical field of view of `fov` degrees.
pub(super) struct Camera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub fov: f32,
}

/// The box of blocks the camera sees: world corners `lo` and `hi`, the animation phase, and the
/// base colour of its faces.
pub(super) struct Scene {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub phase: f32,
    pub base: [f32; 4],
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            lo: [0.0; 3],
            hi: [1.0; 3],
            phase: 0.0,
            base: [0.5, 0.5, 0.5, 1.0],
        }
    }
}

/// A WGSL `vec3<f32>` literal.
fn vec3(v: [f32; 3]) -> String {
    format!("vec3<f32>({:?}, {:?}, {:?})", v[0], v[1], v[2])
}

/// The marker colour of pixels whose ray misses the box: magenta with zero coverage.
pub(super) const MISS: [u8; 4] = [255, 0, 255, 0];

/// Rasters `shade` over `scene` seen by `camera`; returns RGBA8 rows, [`MISS`] off the box.
pub(super) fn view(
    gpu: &Gpu,
    shade: &str,
    textures: &[wgpu::TextureView],
    params: &[f32],
    camera: &Camera,
    scene: &Scene,
) -> Vec<u8> {
    let [r, g, b, a] = scene.base;
    let half = (camera.fov.to_radians() * 0.5).tan();
    raster_with(gpu, shade, textures, params, |library| {
        format!(
            "{library}
struct FxVertex {{ @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }}
@vertex fn fx_vertex(@builtin(vertex_index) index: u32) -> FxVertex {{
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return FxVertex(vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0), vec2<f32>(corner.x, 1.0 - corner.y));
}}
@fragment fn fx_fragment(vertex: FxVertex) -> @location(0) vec4<f32> {{
    let eye = {eye};
    let forward = normalize({target} - eye);
    let right = normalize(cross(forward, vec3<f32>(0.0, 1.0, 0.0001)));
    let up = cross(right, forward);
    let ndc = vec2<f32>(vertex.uv.x * 2.0 - 1.0, 1.0 - vertex.uv.y * 2.0);
    let dir = normalize(forward + (right * ndc.x + up * ndc.y) * {half:?});
    let lo = {lo};
    let hi = {hi};
    let inverse = 1.0 / select(dir, vec3<f32>(1e-6), abs(dir) < vec3<f32>(1e-6));
    let a = (lo - eye) * inverse;
    let b = (hi - eye) * inverse;
    let near3 = min(a, b);
    let near = max(max(near3.x, near3.y), near3.z);
    let far = min(min(max(a.x, b.x), max(a.y, b.y)), max(a.z, b.z));
    if far < max(near, 0.0) || near < 0.0 {{
        return vec4<f32>(1.0, 0.0, 1.0, 0.0);
    }}
    var normal = vec3<f32>(0.0, 0.0, -sign(dir.z));
    if near == near3.x {{
        normal = vec3<f32>(-sign(dir.x), 0.0, 0.0);
    }} else if near == near3.y {{
        normal = vec3<f32>(0.0, -sign(dir.y), 0.0);
    }}
    let world = eye + dir * near;
    let local = world - floor(world - normal * 0.001);
    var uv = local.xy;
    if abs(normal.y) > 0.5 {{
        uv = local.xz;
    }} else if abs(normal.x) > 0.5 {{
        uv = vec2<f32>(local.z, local.y);
    }}
    let input = WorldMaterialInput(uv, world,
        vec4<f32>({r:?}, {g:?}, {b:?}, {a:?}), vec4<f32>(1.0), vec4<f32>(0.0), vec4<f32>(0.0),
        {phase:?}, normal, -dir);
    let surface = world_material_resolve(input);
    return vec4<f32>(surface.color + surface.emission, surface.coverage);
}}
",
            eye = vec3(camera.eye),
            target = vec3(camera.target),
            lo = vec3(scene.lo),
            hi = vec3(scene.hi),
            phase = scene.phase,
        )
    })
}
