// Void walker: the mob's body is a window into deep space. Stars and nebula hang on the view
// ray's direction, so they stay put in the world while the body moves across them, quantized
// to a pixel grid of directions; where the surface turns away from the viewer a rim of pixel
// stars glows. Texels of the skin keep their own coverage, so the silhouette is the mob's.
// params 0-2: rim colour; param 3: star density (0 to 1).

// The star at direction cell `cell`: brightness, twinkling with time.
fn star(cell: vec3<f32>, time: f32, density: f32) -> f32 {
    let h = hash3(cell);
    let lit = h > 1.0 - density;
    let twinkle = 0.6 + 0.4 * sin(time * (2.0 + h * 5.0) + h * 40.0);
    return select(0.0, twinkle, lit);
}

fn shade(input: WorldMaterialInput) -> Surface {
    if input.base.a < 0.5 {
        return Surface(vec3<f32>(0.0), 0.0, vec3<f32>(0.0));
    }
    let time = seconds(input);
    let dir = -input.view;
    let density = clamp(param(input, 3u), 0.0, 1.0) * 0.07;
    // Two star layers on direction grids of different pitch, both fixed in the world.
    let near = star(floor(dir * 90.0), time, density);
    let far = star(floor(dir * 180.0) + 7.0, time * 0.7, density * 0.8) * 0.6;
    let cloud = hash3(floor(dir * 6.0)) * 0.5 + hash3(floor(dir * 12.0) + 3.0) * 0.5;
    let nebula = posterize(mix(vec3<f32>(0.08, 0.0, 0.18), vec3<f32>(0.35, 0.05, 0.4), cloud * cloud), 6.0);
    // The rim: texels where the surface faces away, sparkling on the skin's 64-texel grid.
    let facing = abs(dot(input.normal, input.view));
    let skin_texel = floor(input.uv * 64.0);
    let sparkle = hash3(vec3<f32>(skin_texel, floor(time * 6.0)));
    let rim = select(0.0, 1.0, facing < 0.35) * (0.6 + 0.4 * sparkle);
    let rim_colour = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let space = nebula * 0.6 + vec3<f32>(near + far);
    return Surface(vec3<f32>(0.0), 1.0, space + rim_colour * rim * 2.5);
}
