// Star portal: a 2x3 frame of obsidian opening onto a vortex tunnel. Each pixel's view ray is
// traced into a tunnel running away from the portal's face; its wall spirals in posterized
// violet bands that twist towards a blazing eye, pixel sparks stream inward across the opening,
// and stars from texture 0 sit deep behind. The closer the player, as the server reports, the
// faster it turns and the brighter it burns. The client part keeps the turn's phase continuous
// when the speed changes, so walking closer speeds it up from where it is, never jumping.
// params 0-2: the portal's centre; param 3: its facing in quarter turns from south; param 4: the
// player's closeness, 1 at the face to 0 twelve blocks away; params 5-7: tunnel colour; param 8:
// turns per second; param 9: the turn's phase at session second zero.

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let centre = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let angle = floor(param(input, 3u) + 0.5) * TAU / 4.0;
    let facing = vec3<f32>(-sin(angle), 0.0, cos(angle));
    let right = vec3<f32>(cos(angle), 0.0, sin(angle));
    let up = vec3<f32>(0.0, 1.0, 0.0);
    // The portal faces the player who built it; its other faces are obsidian.
    let rel = input.world_position - centre;
    let face = vec2<f32>(dot(rel, right), rel.y);
    let edge = abs(face.x) > 1.0 - 1.0 / 16.0 || abs(face.y) > 1.5 - 1.0 / 16.0;
    if dot(input.normal, -facing) < 0.5 || edge {
        let texel = floor(input.world_position * 16.0);
        let sheen = select(0.0, 0.3, hash3(texel) > 0.94);
        return Surface(vec3<f32>(0.08, 0.04, 0.14) * (0.8 + 0.4 * hash3(floor(texel / 2.0))), 1.0, vec3<f32>(0.25, 0.1, 0.4) * sheen);
    }
    let near = clamp(param(input, 4u), 0.0, 1.0);
    let spin = time * param(input, 8u) + param(input, 9u);
    let d = -input.view;
    let inward = max(dot(d, facing), 0.05);
    let o = vec2<f32>(face.x, face.y * 0.75);
    let dir = vec2<f32>(dot(d, right), dot(d, up) * 0.75) / inward;
    // Where the ray meets the tunnel wall, radius 1.15: solve |o + dir t| = r for t > 0.
    let a = max(dot(dir, dir), 1e-5);
    let b = dot(o, dir);
    let c = dot(o, o) - 1.15 * 1.15;
    let t = (-b + sqrt(max(b * b - a * c, 0.0))) / a;
    let wall = o + dir * t;
    let depth = t;
    let theta = atan2(wall.y, wall.x);
    let around = floor((theta / TAU * 48.0 + depth * 6.0 - spin * 8.0));
    let along = floor(depth * 16.0);
    let bands = fract(around / 6.0) < 0.5;
    let base = vec3<f32>(param(input, 5u), param(input, 6u), param(input, 7u));
    let fade = clamp((depth - 2.0) / 10.0, 0.0, 1.0);
    var colour = posterize(mix(base * select(0.55, 1.0, bands), vec3<f32>(1.0, 0.9, 1.0), fade * fade), 5.0);
    let star = texture(0u, fract((around + along * 0.37) / vec2<f32>(48.0, 23.0))).r;
    colour = colour + vec3<f32>(star * select(0.0, 1.2, fade < 0.6));
    // Sparks on the opening, spiralling inward in polar texels.
    let r = length(face);
    let polar = floor(vec2<f32>(r * 16.0 + spin * 6.0, (atan2(face.y, face.x) / TAU) * 64.0 + r * 10.0 - spin * 3.0));
    let spark = hash2(polar) > 0.97 - near * 0.03;
    let eye = smoothstep(9.0, 16.0, depth);
    let glow = colour * (0.8 + near * 1.6) + vec3<f32>(1.0, 0.8, 1.0) * eye * 2.0;
    let sparks = select(vec3<f32>(0.0), vec3<f32>(1.0, 0.85, 1.0) * 2.5, spark);
    return Surface(colour * 0.2, 1.0, glow + sparks);
}
