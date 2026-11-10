//! Rendered regressions for terrain prepasses and sky depth bindings.

use super::*;
use crate::shaderpack::draws::DrawKind;
use crate::shaderpack::frame_geometry::TerrainDraws;
use crate::shaderpack::geometry::{
    CONSTANT_VERTEX_BUFFER, DRAW_ORIGIN_BYTES, DRAW_ORIGIN_VERTEX_BUFFER, GEOMETRY_VERTEX_BUFFER,
    GeometryCategory, TerrainLayout,
};

/// Generates a full-screen triangle at a fixed OpenGL clip depth. The explicit `int` cast
/// keeps the vertex index signed, since the GLSL front end types it as `uint`.
fn triangle_vertex(depth: f32) -> String {
    format!(
        "#version 130
void main() {{
    int corner = int(gl_VertexID) % 3;
    gl_Position = vec4(corner == 1 ? 3.0 : -1.0,
                       corner == 2 ? 3.0 : -1.0, {depth}, 1.0);
}}"
    )
}

/// Supplies overlapping opaque triangles; their programs choose the two depths.
struct OverlappingTerrain {
    layout: TerrainLayout,
    vertices: wgpu::Buffer,
}

impl TerrainDraws for OverlappingTerrain {
    /// Draws only solid and cutout categories, with every declared vertex slot bound.
    fn draw<'w>(
        &'w self,
        pass: &mut bevy::render::render_phase::TrackedRenderPass<'w>,
        category: GeometryCategory,
        shadow: bool,
    ) -> u32 {
        if shadow || category == GeometryCategory::Translucent {
            return 0;
        }
        let raw = pass.wgpu_pass();
        for slot in [
            GEOMETRY_VERTEX_BUFFER,
            CONSTANT_VERTEX_BUFFER,
            DRAW_ORIGIN_VERTEX_BUFFER,
        ] {
            raw.set_vertex_buffer(slot, self.vertices.slice(..));
        }
        raw.draw(0..3, 0..1);
        1
    }

    /// Uses the layout chosen by the loaded fixture pack.
    fn layout(&self) -> TerrainLayout {
        self.layout
    }
}

#[test]
fn cutout_prepass_preserves_solid_outputs_it_does_not_replace() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let solid_vertex = triangle_vertex(0.5);
    let cutout_vertex = triangle_vertex(-0.5);
    let source = pack(&[
        ("shaders.properties", "sun=false\nmoon=false\nstars=false\n"),
        ("gbuffers_basic.vsh", &solid_vertex),
        ("gbuffers_basic.fsh", FILL),
        ("gbuffers_terrain_solid.vsh", &solid_vertex),
        (
            "gbuffers_terrain_solid.fsh",
            "#version 120
/* DRAWBUFFERS:01 */
const vec4 colortex1ClearColor = vec4(0.0);
void main() {
    gl_FragData[0] = vec4(1.0, 0.0, 0.0, 1.0);
    gl_FragData[1] = vec4(0.0, 1.0, 0.0, 1.0);
}",
        ),
        ("gbuffers_terrain_cutout.vsh", &cutout_vertex),
        (
            "gbuffers_terrain_cutout.fsh",
            "#version 120\nvoid main() { gl_FragData[0] = vec4(0.0, 0.0, 1.0, 1.0); }",
        ),
        ("final.vsh", PASS_VERTEX),
        (
            "final.fsh",
            "#version 120
uniform sampler2D colortex0;
uniform sampler2D colortex1;
uniform sampler2D depthtex0;
varying vec2 texcoord;
void main() {
    gl_FragColor = vec4(texture2D(colortex0, texcoord).b,
                        texture2D(colortex1, texcoord).g,
                        texture2D(depthtex0, texcoord).r, 1.0);
}",
        ),
    ]);
    let pack = Arc::new(
        build(
            &source,
            "terrain prepass fixture",
            JAVA_DIMENSIONS[0],
            &Default::default(),
            None,
            &gpu,
            &Default::default(),
            &Default::default(),
        )
        .expect("fixture loads"),
    );
    for kind in [DrawKind::TerrainSolid, DrawKind::TerrainCutout] {
        assert!(
            pack.draws
                .iter()
                .any(|draw| draw.kind == kind && draw.prepass.is_some()),
            "fixture must exercise the {kind:?} prepass"
        );
    }
    let terrain = OverlappingTerrain {
        layout: pack.terrain_layout,
        vertices: gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("procedural terrain vertex slots"),
            size: 3 * pack.terrain_layout.stride().max(DRAW_ORIGIN_BYTES),
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        }),
    };
    let (_, bytes) = render_loaded_pack(
        &gpu,
        pack,
        1,
        [0, 0, SIZE[0], SIZE[1]],
        false,
        Some(&terrain),
    );
    assert_pixels(&bytes, |_| [255, 255, 64]);
}

#[test]
fn sky_can_sample_geometry_depth_when_terrain_covers_its_outputs() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let vertex = triangle_vertex(0.5);
    // The first frame leaves far depth for the second frame's sky to sample.
    let (pack, bytes) = render_frame_options(
        &gpu,
        &[
            ("shaders.properties", "sun=false\nmoon=false\nstars=false\n"),
            ("gbuffers_basic.vsh", &vertex),
            (
                "gbuffers_basic.fsh",
                "#version 120\nvoid main() { gl_FragData[0] = vec4(0.0, 0.0, 0.0, 1.0); }",
            ),
            ("gbuffers_skybasic.vsh", &vertex),
            (
                "gbuffers_skybasic.fsh",
                "#version 120
uniform sampler2D depthtex0;
void main() {
    gl_FragData[0] = vec4(vec3(texture2D(depthtex0, vec2(0.5)).r), 1.0);
}",
            ),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
        2,
        [0, 0, SIZE[0], SIZE[1]],
        false,
        true,
    );
    assert!(
        pack.draws
            .iter()
            .any(|draw| draw.kind == DrawKind::Sky && draw.behind_terrain.is_some()),
        "fixture must provide the depth-tested sky variant"
    );
    assert_pixels(&bytes, |_| [255; 3]);
}
