use super::*;

#[test]
fn ui_model_depth_scopes_are_ordered_private_and_cannot_reenter_after_overlay() {
    let mut input = fixture_draw_list(1);
    let mut batches = input.batches.to_vec();
    batches[0] = batches[0]
        .with_depth_test(true)
        .with_depth_write(true)
        .with_isolated_depth_scope(Some(7));
    batches[1] = batches[1]
        .with_depth_test(true)
        .with_isolated_depth_scope(Some(7));
    input.batches = batches.clone().into();
    input.validate().unwrap();
    assert_eq!(input.batches[0].world_projection, 0);
    batches[0].world_projection = 1;
    input.batches = batches.clone().into();
    assert_eq!(
        input.validate(),
        Err(UiRenderRejectReason::UnsupportedWorldProjection { batch: 0 })
    );
    batches[0].world_projection = 0;
    batches[1] = batches[1]
        .with_depth_test(false)
        .with_world_projection(false)
        .with_isolated_depth_scope(None);
    batches[2] = batches[2].with_isolated_depth_scope(Some(7));
    input.batches = batches.into();
    assert_eq!(
        input.validate(),
        Err(UiRenderRejectReason::InvalidIsolatedDepthScope { batch: 2 })
    );
}

#[test]
fn ui_model_material_cutoff_is_finite_and_independent_of_glyph_half_alpha() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].alpha_cutoff = 0.1;
    input.vertices = vertices.clone().into();
    input.validate().unwrap();
    vertices[0].alpha_cutoff = f32::NAN;
    input.vertices = vertices.into();
    assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
    let source = ui_render::shader::source(include_str!("../../src/ui.wgsl"));
    assert!(source.contains("if input.alpha_cutoff >= 0.0"));
    assert!(source.contains("sample.a < input.alpha_cutoff"));
    assert!(source.contains("else if (input.style_flags & STYLE_ALPHA_TEST)"));
}

#[test]
fn ui_model_light_preserves_float_vertex_values_and_rejects_invalid_multipliers() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].model_light = 0.718_629;
    input.vertices = vertices.clone().into();
    input.validate().unwrap();
    assert_eq!(
        input.vertices[0].model_light.to_bits(),
        0.718_629_f32.to_bits()
    );
    for value in [f32::NAN, f32::INFINITY, -0.1] {
        vertices[0].model_light = value;
        input.vertices = vertices.clone().into();
        assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
    }
    let source = ui_render::shader::source(include_str!("../../src/ui.wgsl"));
    assert!(source.contains("@location(5) model_light: f32"));
    assert!(source.contains("straight_color.a * input.model_light"));
}

#[test]
fn ui_model_fire_overlay_is_float_and_rejects_non_finite_channels() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].overlay_color = [0.8, 0.248_176, 0.0, 0.7];
    input.vertices = vertices.clone().into();
    input.validate().unwrap();
    assert_eq!(input.vertices[0].overlay_color, vertices[0].overlay_color);
    for channel in 0..4 {
        vertices[0].overlay_color[channel] = f32::NAN;
        input.vertices = vertices.clone().into();
        assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
        vertices[0].overlay_color[channel] = 0.0;
    }
}

#[test]
fn ui_model_uv_keeps_native_side_pixel_centers_without_rounding_or_half_texel_shift() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].uv = [8.5, 4.5];
    input.vertices = vertices.clone().into();
    input.validate().unwrap();
    assert_eq!(input.vertices[0].uv, [8.5, 4.5]);
    assert_eq!(
        render_model::project_ui_model_vertex(input.vertices[0], None)
            .unwrap()
            .vertex
            .uv,
        [8.5, 4.5]
    );
    vertices[0].uv[1] = f32::NAN;
    input.vertices = vertices.into();
    assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
    let pipeline = ui_pipeline_descriptor(ui_bind_group_layout());
    let uv = pipeline.vertex.buffers[0]
        .attributes
        .iter()
        .find(|attribute| attribute.shader_location == 1)
        .unwrap();
    assert_eq!(uv.format, VertexFormat::Float32x2);
    assert_eq!(uv.offset, std::mem::offset_of!(UiRenderVertex, uv) as u64);
}
