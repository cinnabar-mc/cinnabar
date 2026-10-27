// Screen wall: a 64x48 pixel display across a 4x3 wall of blocks, a live radar of the real
// players' positions, which the server sends as parameters. A green phosphor grid with range
// rings, a rolling scanline and flicker; each player is a blinking blip, the nearest to the
// centre drawn as a cross.
// params 0-2: the display's centre; param 3: its facing in quarter turns from south; param 4:
// blocks per pixel; params 5-12: up to four players' x and z; param 13: how many players.

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let centre = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let quarter = floor(param(input, 3u) + 0.5);
    let angle = quarter * TAU / 4.0;
    let facing = vec3<f32>(-sin(angle), 0.0, cos(angle));
    if dot(input.normal, -facing) < 0.5 {
        return Surface(vec3<f32>(0.12, 0.12, 0.14), 1.0, vec3<f32>(0.0));
    }
    let right = vec3<f32>(cos(angle), 0.0, sin(angle));
    let rel = input.world_position - centre;
    let pixel = floor(vec2<f32>(dot(rel, -right) + 2.0, rel.y + 1.5) * 16.0);
    if pixel.x < 2.0 || pixel.y < 2.0 || pixel.x > 61.0 || pixel.y > 45.0 {
        return Surface(vec3<f32>(0.18, 0.18, 0.2), 1.0, vec3<f32>(0.0));
    }
    let middle = vec2<f32>(32.0, 24.0);
    let offset = pixel - middle;
    var light = 0.08;
    if (pixel.x % 8.0) == 0.0 || (pixel.y % 8.0) == 0.0 {
        light = 0.2;
    }
    let ring = length(offset);
    if abs(ring - 10.0) < 0.5 || abs(ring - 20.0) < 0.5 {
        light = 0.35;
    }
    // A sweep line turning once every three seconds, with an afterglow.
    let sweep = fract(time / 3.0) * TAU;
    let bearing = atan2(offset.y, offset.x) + 3.14159265;
    let behind = fract((sweep - bearing) / TAU + 1.0);
    light = light + select(0.0, 0.45 * (1.0 - behind / 0.15), behind < 0.15 && ring < 22.0);
    // Player blips: the server's positions relative to the display, on its pixel grid.
    let scale = max(param(input, 4u), 0.05);
    let count = clamp(floor(param(input, 13u) + 0.5), 0.0, 4.0);
    let blink = select(0.6, 1.0, fract(time * 1.5) < 0.5);
    var blips = 0.0;
    let players = array<vec2<f32>, 4>(
        vec2<f32>(param(input, 5u), param(input, 6u)), vec2<f32>(param(input, 7u), param(input, 8u)),
        vec2<f32>(param(input, 9u), param(input, 10u)), vec2<f32>(param(input, 11u), param(input, 12u)));
    let screen_right = vec2<f32>(-right.x, -right.z);
    let screen_up = vec2<f32>(facing.x, facing.z);
    let p0 = floor(vec2<f32>(dot(players[0] - centre.xz, screen_right), dot(players[0] - centre.xz, screen_up)) / scale) + middle;
    let p1 = floor(vec2<f32>(dot(players[1] - centre.xz, screen_right), dot(players[1] - centre.xz, screen_up)) / scale) + middle;
    let p2 = floor(vec2<f32>(dot(players[2] - centre.xz, screen_right), dot(players[2] - centre.xz, screen_up)) / scale) + middle;
    let p3 = floor(vec2<f32>(dot(players[3] - centre.xz, screen_right), dot(players[3] - centre.xz, screen_up)) / scale) + middle;
    let cross0 = (abs(pixel.x - p0.x) < 0.5 && abs(pixel.y - p0.y) < 2.5) || (abs(pixel.y - p0.y) < 0.5 && abs(pixel.x - p0.x) < 2.5);
    blips = max(blips, select(0.0, 1.0, count > 0.5 && cross0));
    blips = max(blips, select(0.0, blink, count > 1.5 && all(abs(pixel - p1) < vec2<f32>(1.0))));
    blips = max(blips, select(0.0, blink, count > 2.5 && all(abs(pixel - p2) < vec2<f32>(1.0))));
    blips = max(blips, select(0.0, blink, count > 3.5 && all(abs(pixel - p3) < vec2<f32>(1.0))));
    let roll = select(1.0, 1.3, abs((pixel.y + floor(time * 20.0)) % 48.0) < 1.0);
    let flicker = 0.92 + 0.08 * sin(time * 50.0);
    let level = floor(clamp(max(light, blips), 0.0, 1.0) * 5.0 + 0.5) / 5.0 * roll * flicker;
    let phosphor = select(vec3<f32>(0.2, 1.0, 0.35), vec3<f32>(1.0, 0.95, 0.4), blips > 0.5);
    return Surface(vec3<f32>(0.02, 0.05, 0.02), 1.0, phosphor * level * 1.4);
}
