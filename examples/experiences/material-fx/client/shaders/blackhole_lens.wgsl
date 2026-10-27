// Black hole lensing the real world: as the black hole, but the light it bends is what is
// actually behind it on the screen, shifted to where the bent ray would have met it, so the
// gallery, the ground and the sky warp around the shadow. Each face traces its view ray past a
// hole at the cluster's centre. Rays passing within the photon sphere's capture
// radius fall in; the rest bend by an angle that grows sharply towards it, so the procedural
// sky behind is lensed and the accretion disk shows both its near side and, bent over the top
// and under the bottom, its far side. The disk is hotter inwards and Doppler-boosted on the
// side spinning towards the viewer; a thin photon ring circles the shadow. Outside the lensing
// radius the cluster is transparent.
// params 0-2: centre; param 3: horizon radius (blocks); param 4: lensing radius (blocks);
// param 5: pixels per radian of bend.

fn sky(dir: vec3<f32>, time: f32) -> vec3<f32> {
    let band = exp(-abs(dir.y + 0.3 * dir.x) * 6.0);
    let milky = vec3<f32>(0.35, 0.3, 0.5) * band * 0.6;
    let cell = floor(dir * 160.0);
    let star = select(0.0, 1.0, hash3(cell) > 0.993) * (0.7 + 0.3 * sin(time * 3.0 + hash3(cell + 1.0) * 30.0));
    return vec3<f32>(0.004, 0.004, 0.012) + milky + vec3<f32>(star);
}

// The disk's light at `p` (hole frame, on the plane), seen along `dir`; alpha 0 off the disk.
fn disk(p: vec3<f32>, dir: vec3<f32>, horizon: f32, time: f32) -> vec4<f32> {
    let r = length(p.xz);
    let inner = horizon * 2.6;
    let outer = horizon * 7.0;
    if r < inner || r > outer {
        return vec4<f32>(0.0);
    }
    let t = (r - inner) / (outer - inner);
    let angle = atan2(p.z, p.x);
    let spin = time * 1.8 / max(r / horizon, 1.0);
    let swirl = 0.55 + 0.45 * sin(angle * 5.0 - spin * 6.0 + r * 4.0) * sin(angle * 3.0 + spin * 4.0 - r * 7.0);
    let hot = mix(vec3<f32>(1.0, 0.95, 0.85), vec3<f32>(1.0, 0.45, 0.1), smoothstep(0.0, 0.5, t));
    let colour = mix(hot, vec3<f32>(0.6, 0.08, 0.02), smoothstep(0.5, 1.0, t));
    // The orbit carries the gas around +y; the side moving towards the viewer brightens and blues.
    let orbit = normalize(vec3<f32>(-p.z, 0.0, p.x));
    let boost = clamp(1.0 + dot(orbit, -dir) * 0.75, 0.25, 1.9);
    let shifted = colour * mix(vec3<f32>(1.15, 0.8, 0.6), vec3<f32>(0.7, 0.85, 1.3), clamp(boost - 0.6, 0.0, 1.0));
    let fade = smoothstep(1.0, 0.85, t) * smoothstep(0.0, 0.06, t);
    return vec4<f32>(shifted * swirl * boost * boost * boost * 2.2, fade);
}

fn shade(input: WorldMaterialInput) -> Surface {
    let time = seconds(input);
    let centre = vec3<f32>(param(input, 0u), param(input, 1u), param(input, 2u));
    let horizon = max(param(input, 3u), 0.05);
    let reach = max(param(input, 4u), horizon * 3.0);
    let o = input.world_position - centre;
    let d = -input.view;
    let along = -dot(o, d);
    let closest = o + d * along;
    let b = length(closest);
    let coverage = smoothstep(reach, reach * 0.8, b);
    if coverage <= 0.0 {
        return Surface(vec3<f32>(0.0), 0.0, vec3<f32>(0.0));
    }
    let capture = horizon * 2.598;
    var light = vec3<f32>(0.0);
    // The near side of the disk, on the straight run before closest approach.
    let t1 = -o.y / select(d.y, 1e-5, abs(d.y) < 1e-5);
    var front = vec4<f32>(0.0);
    if t1 > 0.0 && t1 < max(along, 0.0) {
        front = disk(o + d * t1, d, horizon, time);
    }
    if b < capture {
        light = vec3<f32>(0.0);
    } else {
        // Bend towards the hole; the bend sharpens at the photon sphere.
        let towards = -closest / b;
        let bend = min(2.0 * horizon / max(b - capture * 0.9, 1e-3), 3.0);
        let bent = normalize(d * cos(bend) + towards * sin(bend));
        light = scene_color(screen_shift(d, bent, param(input, 5u)));
        // The far side of the disk, seen along the bent ray.
        let t2 = -closest.y / select(bent.y, 1e-5, abs(bent.y) < 1e-5);
        if t2 > 0.0 {
            let back = disk(closest + bent * t2, bent, horizon, time);
            light = mix(light, back.rgb, back.a);
        }
        // The photon ring just outside the shadow.
        let ring = exp(-pow((b - capture * 1.04) / (horizon * 0.06), 2.0));
        light = light + vec3<f32>(1.0, 0.85, 0.6) * ring * 2.5;
    }
    light = mix(light, front.rgb, front.a);
    return Surface(vec3<f32>(0.0), coverage, light);
}
