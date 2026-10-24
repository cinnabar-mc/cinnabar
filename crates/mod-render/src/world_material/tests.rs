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
        (frame[0], frame[1], frame[4], frame[5], frame[6]),
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
