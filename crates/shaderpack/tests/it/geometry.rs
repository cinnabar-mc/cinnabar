//! Original geometry fixtures exercise compilation without redistributing pack code.

use super::*;
use shaderpack::{PackSource, StageKind};

#[test]
fn shadow_geometry_compiles_to_bounded_compute_and_vertex_reader() {
    let Some(pack) = open_fixture("geometry") else {
        return;
    };
    let translated = pack.translate(&Default::default());
    let shadow = program(&translated, "shadow");
    reflection(shadow);
    let emulation = shadow.geometry.as_ref().expect("geometry expansion");
    assert_eq!(emulation.max_vertices, 6);
    assert_eq!(emulation.inputs[0].name, "tint");
    let compute = shadow
        .stages
        .iter()
        .find(|s| s.stage == StageKind::Geometry)
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    assert_eq!(
        compute.module.entry_points[0].stage,
        naga::ShaderStage::Compute
    );
    let original = shadow
        .stages
        .iter()
        .find(|s| s.stage == StageKind::Vertex)
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    shaderpack::geometry_vertex_compute(original, emulation, &[]).expect("vertex prepass compiles");
}

#[test]
fn unsupported_geometry_keeps_other_programs_translated() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "shadow.vsh",
            "#version 330\nvoid main(){gl_Position=vec4(0.0);}",
        ),
        (
            "shadow.gsh",
            "#version 330\nlayout(points) in; layout(triangle_strip,max_vertices=3) out;void main(){EmitVertex();}",
        ),
        (
            "shadow.fsh",
            "#version 330\nout vec4 c;void main(){c=vec4(1.0);}",
        ),
        (
            "final.vsh",
            "#version 330\nvoid main(){gl_Position=vec4(0.0);}",
        ),
        (
            "final.fsh",
            "#version 330\nout vec4 c;void main(){c=vec4(1.0);}",
        ),
    ]));
    let translated = pack.translate(&Default::default());
    assert!(!program(&translated, "shadow").is_ok());
    assert!(
        program(&translated, "shadow")
            .all_diagnostics()
            .any(|d| d.message.contains("triangles input"))
    );
    reflection(program(&translated, "final"));
}

#[test]
fn explicit_geometry_locations_link_different_names_and_split_layouts() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "shadow.vsh",
            "#version 450 core\nlayout(location=0) out vec4 vsTint;\nvoid main(){gl_Position=vec4(0.0);vsTint=vec4(1.0);}",
        ),
        (
            "shadow.gsh",
            "#version 450 core\nlayout(triangles) in;layout(triangle_strip) out;layout(max_vertices=3) out;layout(location=0) in vec4 gsTint[];out vec4 tint;\nvoid main(){for(int i=0;i<3;i++){gl_Position=gl_in[i].gl_Position;tint=gsTint[i];EmitVertex();}EndPrimitive();}",
        ),
        (
            "shadow.fsh",
            "#version 450 core\nin vec4 tint;out vec4 color;void main(){color=tint;}",
        ),
    ]));
    let translated = pack.translate(&Default::default());
    let shadow = program(&translated, "shadow");
    reflection(shadow);
    assert_eq!(shadow.geometry.as_ref().unwrap().max_vertices, 3);
}

#[test]
fn full_screen_geometry_is_reported_without_running_a_partial_program() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "composite.vsh",
            "#version 330\nvoid main(){gl_Position=vec4(0.0);}",
        ),
        (
            "composite.gsh",
            "#version 330\nlayout(triangles) in;layout(triangle_strip,max_vertices=3) out;void main(){for(int i=0;i<3;i++){gl_Position=gl_in[i].gl_Position;EmitVertex();}}",
        ),
        (
            "composite.fsh",
            "#version 330\nout vec4 color;void main(){color=vec4(1.0);}",
        ),
        (
            "final.vsh",
            "#version 330\nvoid main(){gl_Position=vec4(0.0);}",
        ),
        (
            "final.fsh",
            "#version 330\nout vec4 color;void main(){color=vec4(1.0);}",
        ),
    ]));
    let translated = pack.translate(&Default::default());
    let composite = program(&translated, "composite");
    assert!(!composite.is_ok());
    assert!(
        composite
            .all_diagnostics()
            .any(|d| d.message.contains("world and shadow programs"))
    );
    reflection(program(&translated, "final"));
}

#[test]
fn geometry_inputs_link_explicit_and_implicit_vertex_outputs() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "shadow.vsh",
            "#version 450 core\nlayout(location=0) out vec4 a,b;out vec4 tint;void main(){gl_Position=vec4(0.0);a=vec4(1.0);b=vec4(2.0);tint=vec4(3.0);}",
        ),
        (
            "shadow.gsh",
            "#version 450 core\nlayout(triangles) in;layout(triangle_strip,max_vertices=3) out;layout(location=0) in vec4 a[],b[];layout(location=5) in vec4 tint[];out vec4 color;void main(){for(int i=0;i<3;i++){gl_Position=gl_in[i].gl_Position;color=a[i]+b[i]+tint[i];EmitVertex();}}",
        ),
        (
            "shadow.fsh",
            "#version 450 core\nin vec4 color;out vec4 result;void main(){result=color;}",
        ),
    ]));
    let translated = pack.translate(&Default::default());
    let shadow = program(&translated, "shadow");
    reflection(shadow);
    assert_eq!(
        shadow
            .geometry
            .as_ref()
            .unwrap()
            .inputs
            .iter()
            .map(|field| field.location)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(5)]
    );
}

#[test]
fn geometry_output_size_expressions_are_reported_instead_of_truncated() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "shadow.vsh",
            "#version 330\nvoid main(){gl_Position=vec4(0.0);}",
        ),
        (
            "shadow.gsh",
            "#version 330\nlayout(triangles) in;layout(triangle_strip,max_vertices=3*2) out;void main(){for(int i=0;i<6;i++){gl_Position=gl_in[i%3].gl_Position;EmitVertex();}}",
        ),
        (
            "shadow.fsh",
            "#version 330\nout vec4 color;void main(){color=vec4(1.0);}",
        ),
    ]));
    let translated = pack.translate(&Default::default());
    let shadow = program(&translated, "shadow");
    assert!(!shadow.is_ok());
    assert!(shadow.all_diagnostics().any(|diagnostic| {
        diagnostic
            .message
            .contains("max_vertices requires a literal")
    }));
}

#[test]
fn adding_geometry_preserves_the_programs_sampler_normalization() {
    for profile in ["330", "330 core", "330 compatibility"] {
        let files = [
            (
                "shadow.vsh".to_owned(),
                format!("#version {profile}\nvoid main(){{gl_Position=vec4(0.0);}}"),
            ),
            (
                "shadow.fsh".to_owned(),
                format!(
                    "#version {profile}\nuniform sampler2D gcolor;out vec4 color;void main(){{color=texture(gcolor,vec2(0.5));}}"
                ),
            ),
        ];
        let plain = ShaderPack::new(PackSource::from_files(files.clone()));
        let mut files = files.to_vec();
        files.push(("shadow.gsh".to_owned(),format!("#version {profile}\nlayout(triangles) in;layout(triangle_strip,max_vertices=3) out;void main(){{for(int i=0;i<3;i++){{gl_Position=gl_in[i].gl_Position;EmitVertex();}}}}")));
        let geometry = ShaderPack::new(PackSource::from_files(files));
        let plain = plain.translate(&Default::default());
        let geometry = geometry.translate(&Default::default());
        let samplers = |translation: &shaderpack::PackTranslation| {
            reflection(program(translation, "shadow"))
                .textures
                .iter()
                .map(|texture| texture.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(samplers(&geometry), samplers(&plain), "{profile}");
    }
}

#[test]
fn inactive_geometry_inputs_need_no_vertex_producer() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "shadow.vsh",
            "#version 330\nvoid main(){gl_Position=vec4(0.0);}",
        ),
        (
            "shadow.gsh",
            "#version 330\nlayout(triangles) in;layout(triangle_strip,max_vertices=3) out;in vec4 unused[];void main(){for(int i=0;i<3;i++){gl_Position=gl_in[i].gl_Position;EmitVertex();}}",
        ),
        (
            "shadow.fsh",
            "#version 330\nout vec4 color;void main(){color=vec4(1.0);}",
        ),
    ]));
    let translated = pack.translate(&Default::default());
    let shadow = program(&translated, "shadow");
    reflection(shadow);
    assert!(shadow.geometry.as_ref().unwrap().inputs.is_empty());
}

#[test]
fn geometry_interfaces_use_the_same_identifier_rewrites() {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "shadow.vsh",
            "#version 330\nout vec4 texture;void main(){gl_Position=vec4(0.0);texture=vec4(0.25);}",
        ),
        (
            "shadow.gsh",
            "#version 330\nlayout(triangles) in;layout(triangle_strip,max_vertices=3) out;in vec4 texture[];out vec4 color;void main(){for(int i=0;i<3;i++){gl_Position=gl_in[i].gl_Position;color=texture[i];EmitVertex();}}",
        ),
        (
            "shadow.fsh",
            "#version 330\nin vec4 color;out vec4 result;void main(){result=color;}",
        ),
    ]));
    let translated = pack.translate(&Default::default());
    reflection(program(&translated, "shadow"));
}
