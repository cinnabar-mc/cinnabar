// Conditional terrain markers: barrier and light-block sprites that face the camera, and
// structure-void boxes. Lit by their own cell and fogged like ordinary terrain, in gamma RGB.
import bevy_render::view::View;
import render::lighting::{terrain_light_levels, terrain_light_colour, world_fog_amount, world_atmosphere, tint_to_gamma, tint_to_linear};
import render::world_projection::{section_camera_offset, camera_offset_clip};

// The frames of one marker's art, selected by the shared terrain animation clock.
struct MarkerAnimation { frame_start: u32, frame_count: u32, ticks_per_frame: u32, flags: u32 }
// One frame's texture page, array layer, exposed UV scale and source-to-page size ratio.
struct MarkerTexture { page: u32, layer: u32, uv_scale: f32, reserved: u32, gradient_scale: vec2<f32> }
struct MarkerClock { tick: u32, partial_tick: f32, reserved_a: u32, reserved_b: u32 }

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> marker_animations: array<MarkerAnimation>;
@group(0) @binding(2) var<storage, read> marker_textures: array<MarkerTexture>;
@group(0) @binding(3) var<uniform> clock: MarkerClock;
@group(0) @binding(4) var page_0: texture_2d_array<f32>;
@group(0) @binding(5) var page_1: texture_2d_array<f32>;
@group(0) @binding(6) var page_2: texture_2d_array<f32>;
@group(0) @binding(7) var page_3: texture_2d_array<f32>;
@group(0) @binding(8) var page_4: texture_2d_array<f32>;
@group(0) @binding(9) var marker_sampler: sampler;

// Texels at or below this alpha are holes; every other texel draws opaque.
const MARKER_ALPHA_THRESHOLD: f32 = 0.5 / 255.0;

struct VertexOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Bits 8..16 hold the cell light.
    @location(1) @interpolate(flat) packed: u32,
    // Page and layer of the current frame, then of the frame blended towards.
    @location(2) @interpolate(flat) frames: vec4<u32>,
    // UV scales of both frames and the blend weight towards the second.
    @location(3) @interpolate(flat) frame_mix: vec3<f32>,
    // Source-to-page gradient scales of both frames.
    @location(4) @interpolate(flat) gradient_scales: vec4<f32>,
    @location(5) camera_offset: vec3<f32>,
}

@vertex
fn marker_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) block: vec3<i32>,
    // Bit 0 selects the box, bits 8..16 hold the cell light.
    @location(1) packed: u32,
    // Each face's art index in west, east, down, up, north, south order; a sprite uses the
    // first.
    @location(2) art_low: vec3<u32>,
    @location(3) art_high: vec3<u32>,
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
    let corner = corners[vertex_index % 6u];
    var local: vec3<f32>;
    var animation_index = art_low.x;
    if ((packed & 1u) != 0u) {
        let face_index = min(vertex_index / 6u, 5u);
        let face = faces[face_index];
        local = face[0] + face[1] * corner.x + face[2] * corner.y;
        if (face_index < 3u) {
            animation_index = art_low[face_index];
        } else {
            animation_index = art_high[face_index - 3u];
        }
    } else {
        // A one-block sprite centred in its cell, spanning the view's right and up axes.
        let right = view.world_from_view[0].xyz;
        let up = view.world_from_view[1].xyz;
        local = vec3(0.5) + right * (corner.x - 0.5) + up * (corner.y - 0.5);
    }
    var out: VertexOutput;
    let animation = marker_animations[animation_index];
    if (animation.frame_count == 0u) {
        // A face without art collapses to a point and draws nothing.
        out.clip_position = vec4(0.0);
        return out;
    }
    let offset = section_camera_offset(block, local, view.world_position);
    out.clip_position = camera_offset_clip(view.clip_from_world, view.world_position, offset);
    out.uv = vec2(corner.x, 1.0 - corner.y);
    out.packed = packed;
    // Flipbooks step and blend exactly as terrain materials do.
    let ticks = max(animation.ticks_per_frame, 1u);
    let count = animation.frame_count;
    let current = (clock.tick / ticks) % count;
    let blends = (animation.flags & 1u) != 0u && count > 1u;
    let next = select(current, (current + 1u) % count, blends);
    let current_texture = marker_textures[animation.frame_start + current];
    let next_texture = marker_textures[animation.frame_start + next];
    let blend = (f32(clock.tick % ticks) + clamp(clock.partial_tick, 0.0, 0.99999994)) / f32(ticks);
    out.frames = vec4(current_texture.page, current_texture.layer, next_texture.page, next_texture.layer);
    out.frame_mix = vec3(current_texture.uv_scale, next_texture.uv_scale, select(0.0, blend, blends));
    out.gradient_scales = vec4(current_texture.gradient_scale, next_texture.gradient_scale);
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
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let current = in.uv * in.frame_mix.x;
    let next = in.uv * in.frame_mix.y;
    // Gradients follow the source texture's size, as terrain's do, so mips match its resolution.
    let current_gradient = in.gradient_scales.xy;
    let next_gradient = in.gradient_scales.zw;
    var sampled = sample_page(in.frames.x, current, i32(in.frames.y), dx * current_gradient, dy * current_gradient);
    // Explicit gradients let only blending flipbooks take the second sample.
    if (in.frame_mix.z > 0.0) {
        let blended = sample_page(in.frames.z, next, i32(in.frames.w), dx * next_gradient, dy * next_gradient);
        sampled = mix(sampled, blended, in.frame_mix.z);
    }
    if (sampled.a <= MARKER_ALPHA_THRESHOLD) {
        discard;
    }
    let light = terrain_light_colour(terrain_light_levels((in.packed >> 8u) & 255u));
    let fog_gamma = tint_to_gamma(vec4(world_atmosphere.fog_color_start.rgb, 1.0)).rgb;
    let lit = mix(sampled.rgb * light, fog_gamma, world_fog_amount(in.camera_offset, vec3(0.0)));
    return tint_to_linear(vec4(lit, 1.0));
}
