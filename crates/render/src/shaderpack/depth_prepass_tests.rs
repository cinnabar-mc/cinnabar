use super::*;

fn parse(stage: ShaderStage, source: &str) -> Module {
    naga::front::glsl::Frontend::default()
        .parse(&naga::front::glsl::Options::from(stage), source)
        .expect("fixture parses")
}

fn wgsl(module: &Module) -> String {
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(module)
    .expect("module validates");
    naga::back::wgsl::write_string(module, &info, naga::back::wgsl::WriterFlags::empty())
        .expect("module writes")
}

const LIT_TERRAIN: &str = "#version 450
layout(location = 0) out vec4 colour;
layout(location = 1) out vec4 material;
layout(location = 0) in vec4 tint;
layout(location = 1) in vec2 uv;
void main() {
    if (tint.a < 0.5) discard;
    colour = vec4(pow(tint.rgb, vec3(2.2)) * dot(uv, uv), 1.0);
    material = vec4(uv, 0.0, 1.0);
}
";

#[test]
fn the_depth_only_stage_keeps_its_discards_and_drops_every_output() {
    let module = depth_only_fragment(&parse(ShaderStage::Fragment, LIT_TERRAIN))
        .expect("a stage without depth writes has a depth-only form");
    let entry = &module.entry_points[0];
    assert!(entry.function.result.is_none());
    let source = wgsl(&module);
    assert!(source.contains("discard"), "{source}");
    assert!(!source.contains("@location(0) vec4"), "{source}");
}

#[test]
fn a_stage_that_writes_depth_has_no_depth_only_form() {
    let module = parse(
        ShaderStage::Fragment,
        "#version 450
layout(location = 0) out vec4 colour;
void main() {
    colour = vec4(1.0);
    gl_FragDepth = 0.25;
}
",
    );
    assert!(depth_only_fragment(&module).is_none());
}

#[test]
fn the_vertex_position_becomes_invariant() {
    let mut module = parse(
        ShaderStage::Vertex,
        "#version 450
layout(location = 0) in vec3 position;
layout(location = 0) out vec2 uv;
layout(set = 0, binding = 0) uniform Block { mat4 projection; };
void main() {
    uv = position.xy;
    gl_Position = projection * vec4(position, 1.0);
}
",
    );
    assert!(!wgsl(&module).contains("@invariant"));
    invariant_position(&mut module);
    let source = wgsl(&module);
    assert!(source.contains("@invariant"), "{source}");
}

#[test]
fn the_sky_is_culled_only_behind_terrain_that_rewrites_its_buffers() {
    let terrain = [Some(0), Some(6), Some(4)];
    assert!(overwrites(&terrain, &[Some(0)]));
    assert!(overwrites(&terrain, &[None, Some(6)]));
    assert!(!overwrites(&terrain, &[Some(0), Some(5)]));
    assert!(!overwrites(&[Some(6)], &[Some(0)]));
}
