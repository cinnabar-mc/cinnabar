// Conditional terrain markers: barrier and light-block sprites that face the camera, and
// structure-void boxes. Lit by their own cell and fogged like ordinary terrain, in gamma RGB.
import bevy_render::view::View;
import render::lighting::{terrain_light_levels, terrain_light_colour, world_fog_amount, world_atmosphere, tint_to_gamma, tint_to_linear};
import render::world_projection::{section_camera_offset, camera_offset_clip};

struct Marker {
    block: vec3<i32>,
    // Bit 0 selects the box, bits 8..16 hold the cell light, bits 16..19 the texture page.
    packed: u32,
    layer: u32,
    uv_scale: f32,
    reserved: vec2<u32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> markers: array<Marker>;
@group(0) @binding(2) var page_0: texture_2d_array<f32>;
@group(0) @binding(3) var page_1: texture_2d_array<f32>;
@group(0) @binding(4) var page_2: texture_2d_array<f32>;
@group(0) @binding(5) var page_3: texture_2d_array<f32>;
@group(0) @binding(6) var page_4: texture_2d_array<f32>;
@group(0) @binding(7) var marker_sampler: sampler;

// Texels at or below this alpha are holes; every other texel draws opaque.
const MARKER_ALPHA_THRESHOLD: f32 = 0.5 / 255.0;

struct VertexOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) packed: u32,
    @location(2) @interpolate(flat) layer: u32,
    @location(3) camera_offset: vec3<f32>,
}

@vertex
fn marker_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    // Two triangles over a unit quad's corners.
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    // Origin, U axis and V axis of each structure-void face: west, east, down, up, north, south.
    var faces = array<mat3x3<f32>, 6>(
        mat3x3(vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -1.0), vec3(0.0, 1.0, 0.0)),
        mat3x3(vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0)),
        mat3x3(vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0)),
        mat3x3(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0)),
        mat3x3(vec3(1.0, 0.0, 0.0), vec3(-1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0)),
        mat3x3(vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0)),
    );
    let marker = markers[instance_index];
    let corner = corners[vertex_index % 6u];
    var local: vec3<f32>;
    if ((marker.packed & 1u) != 0u) {
        let face = faces[min(vertex_index / 6u, 5u)];
        local = face[0] + face[1] * corner.x + face[2] * corner.y;
    } else {
        // A one-block sprite centred in its cell, spanning the view's right and up axes.
        let right = view.world_from_view[0].xyz;
        let up = view.world_from_view[1].xyz;
        local = vec3(0.5) + right * (corner.x - 0.5) + up * (corner.y - 0.5);
    }
    let offset = section_camera_offset(marker.block, local, view.world_position);
    var out: VertexOutput;
    out.clip_position = camera_offset_clip(view.clip_from_world, view.world_position, offset);
    out.uv = vec2(corner.x, 1.0 - corner.y) * marker.uv_scale;
    out.packed = marker.packed;
    out.layer = marker.layer;
    out.camera_offset = offset;
    return out;
}

// Explicit gradients keep the per-marker page selection out of uniform control flow.
fn sample_page(page: u32, uv: vec2<f32>, layer: i32, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    switch page {
        case 0u: { return textureSampleGrad(page_0, marker_sampler, uv, layer, dx, dy); }
        case 1u: { return textureSampleGrad(page_1, marker_sampler, uv, layer, dx, dy); }
        case 2u: { return textureSampleGrad(page_2, marker_sampler, uv, layer, dx, dy); }
        case 3u: { return textureSampleGrad(page_3, marker_sampler, uv, layer, dx, dy); }
        default: { return textureSampleGrad(page_4, marker_sampler, uv, layer, dx, dy); }
    }
}

@fragment
fn marker_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = sample_page((in.packed >> 16u) & 7u, in.uv, i32(in.layer), dpdx(in.uv), dpdy(in.uv));
    if (sampled.a <= MARKER_ALPHA_THRESHOLD) {
        discard;
    }
    let light = terrain_light_colour(terrain_light_levels((in.packed >> 8u) & 255u));
    let fog_gamma = tint_to_gamma(vec4(world_atmosphere.fog_color_start.rgb, 1.0)).rgb;
    let lit = mix(sampled.rgb * light, fog_gamma, world_fog_amount(in.camera_offset, vec3(0.0)));
    return tint_to_linear(vec4(lit, 1.0));
}
