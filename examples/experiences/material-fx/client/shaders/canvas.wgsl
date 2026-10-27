// Living painting: a 32x32 pixel-art night scene across a 2x2 wall of blocks, texture 0. An owl
// on a branch watches the player: its pupils shift towards wherever the viewer stands and it
// blinks now and then; clouds drift across the sky, stars twinkle and fireflies blink in the
// grass. Texture alpha tags the pixels the shader animates: 1.0 plain, 0.8 sky, 0.6 eye white,
// 0.4 star, 0.2 grass. The painting's other faces show its wooden back.
// params 0-2: the canvas centre; params 3-5: the outward normal of its front.

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let front = normalize(vec3<f32>(param(input, 3u), param(input, 4u), param(input, 5u)));
    if dot(input.normal, front) < 0.5 {
        let grain = floor(local_of(input) * 16.0);
        return Surface(vec3<f32>(0.45, 0.3, 0.16) * (0.8 + 0.2 * hash3(grain)), 1.0, vec3<f32>(0.0));
    }
    let centre = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let right = normalize(cross(-front, vec3<f32>(0.0, 1.0, 0.0)));
    let rel = input.world_position - centre;
    let pixel = clamp(floor(vec2<f32>(dot(rel, right) + 1.0, rel.y + 1.0) * 16.0), vec2<f32>(0.0), vec2<f32>(31.0));
    // A one-pixel gilded frame.
    if pixel.x == 0.0 || pixel.y == 0.0 || pixel.x == 31.0 || pixel.y == 31.0 {
        let shine = select(0.75, 1.0, (pixel.x + pixel.y) % 3.0 == 0.0);
        return Surface(vec3<f32>(0.85, 0.65, 0.2) * shine, 1.0, vec3<f32>(0.05, 0.03, 0.0));
    }
    let art = texture(0u, (vec2<f32>(pixel.x, 31.0 - pixel.y) + 0.5) / 32.0);
    let tag = floor(art.a * 5.0 + 0.5);
    // Declared textures decode as sRGB; the host's surface colour is display-encoded.
    var colour = pow(art.rgb, vec3<f32>(0.4545));
    var glow = vec3<f32>(0.0);
    if tag == 4.0 {
        // Sky: a band of pixel clouds drifting right.
        let drift = floor(pixel.x - time * 1.5);
        let cloud = hash2(vec2<f32>(floor(drift / 3.0), floor(pixel.y / 2.0))) > 0.82 && pixel.y > 20.0;
        colour = select(colour, vec3<f32>(0.32, 0.35, 0.5), cloud);
    } else if tag == 2.0 {
        // Stars.
        let twinkle = 0.5 + 0.5 * sin(time * 3.0 + hash2(pixel) * 20.0);
        glow = colour * twinkle * 0.8;
    } else if tag == 1.0 {
        // Grass with fireflies.
        let fly = hash3(vec3<f32>(pixel, floor(time * 1.3))) > 0.985;
        colour = select(colour, vec3<f32>(0.9, 1.0, 0.3), fly);
        glow = select(vec3<f32>(0.0), vec3<f32>(0.9, 1.0, 0.3), fly);
    } else if tag == 3.0 {
        // The eyes: whites with pupils that follow the viewer, shut while blinking.
        let blink = fract(time / 4.7) < 0.035;
        let look = vec2<f32>(dot(input.view, right), input.view.y);
        let shift = clamp(floor(look * 2.5 + 0.5), vec2<f32>(-1.0), vec2<f32>(1.0));
        let eye_centre = select(vec2<f32>(19.0, 21.0), vec2<f32>(13.0, 21.0), pixel.x < 16.0);
        let pupil = all(pixel == eye_centre + shift);
        colour = select(vec3<f32>(1.0, 0.85, 0.3), vec3<f32>(0.05, 0.03, 0.02), pupil);
        colour = select(colour, vec3<f32>(0.42, 0.3, 0.2), blink);
        glow = select(vec3<f32>(0.25, 0.18, 0.04), vec3<f32>(0.0), pupil || blink);
    }
    return Surface(colour, 1.0, glow + colour * 0.15);
}
