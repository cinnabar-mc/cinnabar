use super::*;

const PLAIN: &str = "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }";

#[test]
fn accepts_host_inputs_declared_textures_and_parameters() {
    compose(PLAIN, 0).unwrap();
    let material = compose(
        "fn shade(input: WorldMaterialInput) -> Surface {
            let glow = texture(0u, input.uv).rgb * param(input, 1u);
            let lit = input.base.rgb * (0.5 + 0.5 * input.light.x) + input.overlay.rgb;
            return Surface(mix(lit, input.fog.rgb, 0.0), input.base.a, glow * input.phase);
        }",
        2,
    )
    .unwrap();
    let library = material.library(3);
    assert!(library.contains("@group(3) @binding(0) var<uniform> world_material_frame"));
    assert!(library.contains("@group(3) @binding(2) var world_material_texture_1"));
    assert!(library.contains("@group(3) @binding(3) var world_material_sampler"));
    let frame = frame_uniform(0.5, 3, &[0.25, 2.0, f32::NAN]);
    assert_eq!(
        (frame[0], frame[1], frame[12], frame[13], frame[14]),
        (0.5, 3.0, 0.25, 2.0, 0.0)
    );
}

/// The hostile fixtures: a guest may not declare resources, storage, module state, entry
/// points or depth output, loop, recurse or exceed the expression budget.
#[test]
fn hostile_sources_are_rejected() {
    for source in [
        format!("@group(1) @binding(0) var stolen: texture_2d<f32>; {PLAIN}"),
        format!("@group(1) @binding(0) var<storage, read_write> values: array<f32>; {PLAIN}"),
        format!("@group(0) @binding(9) var<uniform> host: vec4<f32>; {PLAIN}"),
        format!("var<private> state: f32; {PLAIN}"),
        format!("override knob: f32 = 1.0; {PLAIN}"),
        format!("@fragment fn guest() -> @builtin(frag_depth) f32 {{ return 0.0; }} {PLAIN}"),
        format!("@vertex fn guest() -> @builtin(position) vec4<f32> {{ return vec4<f32>(0.0); }} {PLAIN}"),
        "fn shade(input: WorldMaterialInput) -> Surface { loop {} return surface(input); }".into(),
        "fn shade(input: WorldMaterialInput) -> Surface { return shade(input); }".into(),
        "fn shade(input: WorldMaterialInput) -> vec4<f32> { return input.base; }".into(),
        // A third texture when only two were declared does not exist.
        "fn shade(input: WorldMaterialInput) -> Surface { return Surface(textureSampleLevel(world_material_texture_2, world_material_sampler, input.uv, 0.0).rgb, 1.0, vec3<f32>(0.0)); }".into(),
    ] {
        assert!(compose(&source, 2).is_err(), "accepted {source}");
    }
    let mut source = "fn f0(x: f32) -> f32 { return x*x; }".to_owned();
    for index in 1..14 {
        let previous = index - 1;
        source.push_str(&format!(
            "fn f{index}(x: f32) -> f32 {{ return f{previous}(x)+f{previous}(x); }}"
        ));
    }
    source.push_str(
        "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(f13(input.uv.x)), 1.0, vec3<f32>(0.0)); }",
    );
    assert!(compose(&source, 0).unwrap_err().contains("expressions"));
    assert!(compose(&" ".repeat(mod_api::MAX_MATERIAL_SHADER_BYTES + 1), 0).is_err());
}

#[test]
fn texture_reads_are_bounded_per_pixel() {
    let reads = (0..=MAX_SHADER_TEXTURE_SAMPLES)
        .map(|_| "texture(0u, input.uv).r")
        .collect::<Vec<_>>()
        .join(" + ");
    let source = format!(
        "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({reads}), 1.0, vec3<f32>(0.0)); }}"
    );
    assert!(compose(&source, 1).unwrap_err().contains("textures"));
}

#[test]
fn a_rim_glow_reads_the_surface_normal_and_view_direction() {
    compose(
        "fn shade(input: WorldMaterialInput) -> Surface {
            let facing = clamp(dot(input.normal, input.view), 0.0, 1.0);
            let rim = pow(1.0 - facing, 3.0);
            return Surface(input.base.rgb, input.base.a, vec3<f32>(0.2, 0.8, 1.0) * rim);
        }",
        0,
    )
    .unwrap();
}

#[test]
fn a_program_is_identified_by_its_source_texture_count_and_features() {
    let material = |source: &str, textures| WorldMaterial {
        source: source.into(),
        textures,
        features: Features::default(),
    };
    let plain = material(PLAIN, 0).program();
    assert_eq!(
        material(PLAIN, 0).program(),
        plain,
        "equal programs share it"
    );
    assert_ne!(material(PLAIN, 1).program(), plain);
    assert_ne!(material(&format!("{PLAIN} "), 0).program(), plain);
    let displacing = WorldMaterial {
        features: Features {
            displace: true,
            ..Features::default()
        },
        ..material(PLAIN, 0)
    };
    assert_ne!(
        displacing.program(),
        plain,
        "its effect features compose other modules"
    );
}

/// A material using every feature: displacement, the scene, cells, an outline and particles.
const EVERY_FEATURE: &str = "
fn shade(input: WorldMaterialInput) -> Surface {
    let behind = scene_color(vec2<f32>(input.normal.x * 4.0, 0.0));
    let soft = clamp((scene_depth(vec2<f32>(0.0)) - fragment_depth()) / 0.5, 0.0, 1.0);
    return Surface(behind * soft, 0.5, cell(0u).rgb);
}
fn displace(input: WorldVertexInput) -> vec3<f32> {
    return input.normal * sin(input.phase * 100.0 + input.world_position.x) * parameter(0u);
}
fn outline(input: WorldMaterialInput) -> vec3<f32> { return vec3<f32>(1.0, 0.2, 0.1); }
fn particle(input: WorldParticleInput) -> WorldParticle {
    let c = cell(input.index);
    return WorldParticle(c.xyz, 1.0, vec3<f32>(1.0), 1.0);
}
";

fn every() -> Features {
    Features {
        displace: true,
        scene: true,
        cells: true,
        outline: true,
        particles: true,
    }
}

#[test]
fn each_feature_adds_its_functions_and_resources() {
    let material = compose_with(EVERY_FEATURE, 1, every()).unwrap();
    let bindings = material.bindings();
    // Frame 0, texture 1, sampler 2, then the cells; the scene in the next group.
    assert_eq!((bindings.cells, bindings.scene), (Some(3), true));
    let library = material.library(2);
    assert!(library.contains("@group(3) @binding(0) var world_material_scene_color"));
    assert!(library.contains("@group(3) @binding(1) var world_material_scene_depth"));
    assert!(library.contains("@group(2) @binding(3) var<storage, read> world_material_cells"));
    assert!(library.contains("fn world_material_displace"));
    // Without the features the functions still exist and return neutral values, so a
    // withdrawn effect never stops the material compiling.
    let without = compose(EVERY_FEATURE, 1).unwrap();
    assert_eq!(
        without.bindings(),
        MaterialBindings::of(1, Features::default())
    );
    assert!(
        without
            .library(2)
            .contains("fn scene_depth(offset: vec2<f32>) -> f32 { return 1.0e9; }")
    );
    for missing in [
        Features {
            scene: false,
            ..every()
        },
        Features {
            cells: false,
            ..every()
        },
    ] {
        compose_with(EVERY_FEATURE, 1, missing).unwrap();
    }
    // A feature whose function the guest does not define is refused.
    assert!(
        compose_with(
            PLAIN,
            0,
            Features {
                outline: true,
                ..Features::default()
            }
        )
        .is_err()
    );
    assert!(
        compose_with(
            PLAIN,
            0,
            Features {
                displace: true,
                ..Features::default()
            }
        )
        .is_err()
    );
    assert_eq!(MaterialBindings::of(0, Features::default()).cells, None);
    assert_eq!(
        MaterialBindings::of(
            0,
            Features {
                cells: true,
                ..Features::default()
            }
        )
        .cells,
        Some(1)
    );
    let frame = frame_uniform_with_effects(
        0.25,
        2,
        [[0.5, f32::NAN, 2.0, 64.0], [3.0, 1.5, 6.0, 0.0]],
        &[7.0],
    );
    assert_eq!(
        &frame[..13],
        &[
            0.25, 2.0, 0.0, 0.0, 0.5, 0.0, 2.0, 64.0, 3.0, 1.5, 6.0, 0.0, 7.0
        ]
    );
}

#[test]
fn every_feature_function_is_bounded_on_its_own() {
    let heavy = |name: &str, signature: &str, value: &str| {
        let mut source = "fn f0(x: f32) -> f32 { return x*x; }".to_owned();
        for index in 1..14 {
            let previous = index - 1;
            source.push_str(&format!(
                "fn f{index}(x: f32) -> f32 {{ return f{previous}(x)+f{previous}(x); }}"
            ));
        }
        source.push_str(PLAIN);
        source.push_str(&format!("fn {name}({signature} {{ return {value}; }}"));
        source
    };
    let displace = heavy(
        "displace",
        "input: WorldVertexInput) -> vec3<f32>",
        "vec3<f32>(f13(input.phase))",
    );
    let features = Features {
        displace: true,
        ..Features::default()
    };
    assert!(
        compose_with(&displace, 0, features)
            .unwrap_err()
            .contains("expressions")
    );
    let particle = heavy(
        "particle",
        "input: WorldParticleInput) -> WorldParticle",
        "WorldParticle(vec3<f32>(f13(input.phase)), 1.0, vec3<f32>(1.0), 1.0)",
    );
    let features = Features {
        particles: true,
        ..Features::default()
    };
    assert!(
        compose_with(&particle, 0, features)
            .unwrap_err()
            .contains("expressions")
    );
}

#[test]
fn scene_reads_count_as_texture_reads() {
    let reads = (0..=MAX_SHADER_TEXTURE_SAMPLES)
        .map(|_| "scene_color(vec2<f32>(1.0)).r")
        .collect::<Vec<_>>()
        .join(" + ");
    let source = format!(
        "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({reads}), 1.0, vec3<f32>(0.0)); }}"
    );
    let features = Features {
        scene: true,
        ..Features::default()
    };
    assert!(
        compose_with(&source, 0, features)
            .unwrap_err()
            .contains("textures")
    );
}
