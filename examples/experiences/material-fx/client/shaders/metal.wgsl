// Liquid metal mob: mirror chrome reflecting a pixel skybox (texture 0, a 64x32 equirectangular
// panorama), with every skin texel holding one reflection so the chrome stays pixel-crisp. A
// hit sends ripples out from the struck point that bend the reflections, then settle.
// params 0-2: the hit point; param 3: the hit's start in session seconds.

fn shade(input: WorldMaterialInput) -> Surface {
    if input.base.a < 0.5 {
        return Surface(vec3<f32>(0.0), 0.0, vec3<f32>(0.0));
    }
    let texel = floor(input.uv * 64.0);
    let age = since(input, param(input, 3u));
    let reach = distance(input.world_position, vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u)));
    let wave = sin(reach * 18.0 - age * 14.0) * exp(-age * 1.6) * exp(-reach * 1.5);
    let jitter = vec3<f32>(hash2(texel), hash2(texel + 9.0), hash2(texel + 4.0)) - 0.5;
    let normal = normalize(input.normal + jitter * 0.03 + vec3<f32>(wave * 0.6, wave * 0.3, -wave * 0.6));
    let mirror = reflect(-input.view, normal);
    let lon = atan2(mirror.z, mirror.x) / TAU + 0.5;
    let lat = acos(clamp(mirror.y, -1.0, 1.0)) / 3.14159265;
    let sky = texture(0u, (floor(vec2<f32>(lon * 64.0, lat * 32.0)) + 0.5) / vec2<f32>(64.0, 32.0)).rgb;
    let sky_colour = pow(sky, vec3<f32>(0.4545));
    let fresnel = 0.55 + 0.45 * pow(1.0 - abs(dot(normal, input.view)), 3.0);
    let chrome = posterize(sky_colour * fresnel * vec3<f32>(0.92, 0.95, 1.0), 8.0);
    return Surface(chrome * 0.6, 1.0, chrome * 0.9);
}
