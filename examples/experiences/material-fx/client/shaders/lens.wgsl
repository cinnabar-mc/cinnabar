// Lens: a block of real glass. Each texel of its face is a bevelled pane that bends the view
// ray through it, and the face shows the world actually behind it, shifted by that refraction
// on the screen, with a faint cyan tint, a lead frame and a glint. Needs the scene; without it
// the panes show their tint alone.
// param 0: refraction strength (0 to 1); param 1: pixels per radian of bend; param 2: 1 when
// the scene is readable.

fn shade(input: WorldMaterialInput) -> Surface {
    let local = local_of(input);
    let uv = face_uv(local, input.normal);
    let texel = floor(uv * TEXELS);
    let frame = texel.x < 1.0 || texel.y < 1.0 || texel.x > 14.0 || texel.y > 14.0;
    if frame {
        return Surface(vec3<f32>(0.18, 0.18, 0.22), 1.0, vec3<f32>(0.0));
    }
    // Each 4x4 texel pane tilts its own way.
    let pane = floor(texel / 4.0);
    let tilt = vec3<f32>(hash2(pane) - 0.5, hash2(pane + 9.0) - 0.5, hash2(pane + 17.0) - 0.5);
    let normal = normalize(input.normal + tilt * 0.6);
    let d = -input.view;
    let bent = refract(d, normal, 1.0 / 1.5);
    let shift = screen_shift(d, bent, param(input, 1u)) * clamp(param(input, 0u), 0.0, 1.0);
    let behind = select(vec3<f32>(0.3, 0.42, 0.48), scene_color(floor(shift)), param(input, 2u) > 0.5);
    let glint = select(vec3<f32>(0.0), vec3<f32>(0.6), texel.x + texel.y == 6.0 || texel.x + texel.y == 7.0);
    let tint = vec3<f32>(0.85, 0.97, 1.0);
    return Surface(vec3<f32>(0.0), 1.0, behind * tint + glint);
}
