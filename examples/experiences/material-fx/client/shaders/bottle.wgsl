// Ship in a bottle: a glass block holding a tiny voxel world, a floating island with a village,
// a waterfall pouring off its edge and a pixel sun circling it. Each pixel traces its view ray
// through a 16x16x16 voxel grid in texture 0 (sixteen 16x16 slices, slice y at tile
// (y % 4, y / 4)); each voxel's alpha is its Chebyshev distance to the nearest solid voxel in
// voxels / 15, so the trace leaps across empty space and lands on voxel faces, which are the
// block's own texels. Missed rays see through the faint glass.
// param 0: sun orbit period in seconds; param 1: glass tint strength.

struct Trace {
    // Position in voxels, [0, 16] across the block.
    p: vec3<f32>,
    dir: vec3<f32>,
    // The reciprocal of `dir`, for slab exits.
    inv: vec3<f32>,
    // 0 while tracing, 1 once inside a solid voxel, -1 once out of the block.
    hit: f32,
    colour: vec3<f32>,
    cell: vec3<f32>,
}

// One leap: inside a solid voxel the ray stops; otherwise it jumps to where it leaves the
// empty cube around its voxel. A voxel's texel is slice y's tile at (y % 4, y / 4).
fn leap(t: Trace) -> Trace {
    if t.hit != 0.0 {
        return t;
    }
    let cell = floor(min(max(t.p, vec3<f32>(0.0)), vec3<f32>(15.999)));
    let slice = cell.y * 0.25;
    let tile = vec2<f32>(fract(slice) * 4.0, floor(slice));
    let v = texture(0u, (tile * 16.0 + cell.xz + 0.5) * 0.015625);
    let reach = floor(v.a * 15.0 + 0.5);
    if reach < 0.5 {
        return Trace(t.p, t.dir, t.inv, 1.0, v.rgb, cell);
    }
    let far = max((cell - reach + 1.0 - t.p) * t.inv, (cell + reach - t.p) * t.inv);
    let next = t.p + t.dir * (min(min(far.x, far.y), far.z) + 0.001);
    let out = min(min(next.x, next.y), next.z) < 0.0 || max(max(next.x, next.y), next.z) > 16.0;
    return Trace(next, t.dir, t.inv, select(0.0, -1.0, out), t.colour, cell);
}

fn leap4(t: Trace) -> Trace {
    return leap(leap(leap(leap(t))));
}

fn shade(input: WorldMaterialInput) -> Surface {
    let local = local_of(input);
    let dir = -input.view;
    let inv = 1.0 / select(dir, vec3<f32>(1e-6), abs(dir) < vec3<f32>(1e-6));
    var t = Trace(local * 16.0 + dir * 0.002, dir, inv, 0.0, vec3<f32>(0.0), vec3<f32>(0.0));
    t = leap4(leap4(leap4(leap4(leap4(leap4(leap4(t)))))));
    t = leap(leap(t));
    // The sun: a 2-voxel cube circling above the island.
    let period = max(param(input, 0u), 1.0);
    let angle = TAU * fract(seconds(input) / period);
    let sun = vec3<f32>(8.0 + 5.5 * cos(angle), 13.0, 8.0 + 5.5 * sin(angle));
    let start = local * 16.0;
    let sun_hit = box_entry(start, dir, sun - 1.0, sun + 1.0);
    let voxel_distance = select(1e9, length(t.p - start), t.hit > 0.5);
    if sun_hit >= 0.0 && sun_hit < voxel_distance {
        return Surface(vec3<f32>(1.0, 0.86, 0.3), 1.0, vec3<f32>(3.0, 2.2, 0.6));
    }
    // Glass: a one-texel frame along every block edge, a faint tint and a diagonal glint.
    let uv = face_uv(local, input.normal);
    let edge = min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y)) < 1.0 / 16.0;
    // A short two-texel streak of reflected light near one corner of each pane.
    let glint_texel = floor(uv * 16.0);
    let streak = glint_texel.x + (15.0 - glint_texel.y);
    let glint = (streak == 5.0 || streak == 6.0) && glint_texel.x >= 2.0 && glint_texel.x <= 4.0;
    if edge {
        return Surface(vec3<f32>(0.78, 0.88, 0.95), 1.0, vec3<f32>(0.0));
    }
    if t.hit < 0.5 {
        let tint = clamp(param(input, 1u), 0.0, 1.0);
        let shine = select(0.0, 0.45, glint);
        return Surface(vec3<f32>(0.75, 0.9, 1.0), tint + shine, vec3<f32>(shine * 0.3));
    }
    // Declared textures decode as sRGB; the host's surface colour is display-encoded.
    let voxel_colour = pow(t.colour, vec3<f32>(0.4545));
    // Voxel faces lit by the sun's direction in three steps; water ripples down in bands.
    let normal = cell_normal(t.p, t.cell + 0.5);
    let towards = normalize(sun - (t.cell + 0.5));
    let lit = floor((0.5 + 0.5 * dot(normal, towards)) * 3.0 + 0.5) / 3.0;
    var colour = voxel_colour * (0.45 + 0.55 * lit);
    let water = voxel_colour.b > 0.6 && voxel_colour.r < 0.35;
    if water {
        let band = fract((t.cell.y + t.cell.x * 0.5) * 0.25 + seconds(input) * 1.5);
        colour = colour * select(1.0, 1.35, band < 0.25);
    }
    let glow = select(vec3<f32>(0.0), voxel_colour * 0.6, voxel_colour.r > 0.9 && voxel_colour.g > 0.7 && voxel_colour.b < 0.5);
    let glass = select(vec3<f32>(0.0), vec3<f32>(0.25), glint);
    return Surface(min(colour + glass, vec3<f32>(1.0)), 1.0, glow);
}
