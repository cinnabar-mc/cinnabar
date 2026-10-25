// World material bloom and outlines, over what the effects pass drew of the bound targets:
// `emission` holds each covered pixel's emission times its bloom strength, and `outline` its
// outline colour over sixteen and width over eight pixels (alpha zero outside outlined
// targets). A horizontal pass blurs the emission and finds each pixel's nearest outlined pixel
// in its row; the final pass blurs vertically, finishes the outline search within each target's
// width, and blends both over the scene premultiplied.

@group(0) @binding(0) var emission: texture_2d<f32>;
@group(0) @binding(1) var outline: texture_2d<f32>;
// x: bloom radius in pixels; y: the widest outline in pixels.
@group(0) @binding(2) var<uniform> reach: vec4<f32>;
@group(0) @binding(3) var bloom_row: texture_2d<f32>;
@group(0) @binding(4) var outline_row: texture_2d<f32>;

const MAX_BLOOM_RADIUS: i32 = 16;
const MAX_OUTLINE: i32 = 8;
// Outline search results pack the source's width times this plus the distance to it.
const PACK: f32 = 32.0;
// A pixel inside an outlined target, which draws no outline over itself.
const INSIDE: f32 = -2.0;
const NONE: f32 = -1.0;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(corner * 2.0 - vec2(1.0), 0.0, 1.0);
}

fn load(source: texture_2d<f32>, pixel: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    return textureLoad(source, clamp(pixel, vec2(0), size - vec2(1)), 0);
}

fn weight(offset: i32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return select(0.0, 1.0, offset == 0);
    }
    if f32(abs(offset)) > radius {
        return 0.0;
    }
    let sigma = max(radius / 2.0, 0.5);
    let x = f32(offset) / sigma;
    return exp(-0.5 * x * x);
}

struct RowOutput {
    @location(0) bloom: vec4<f32>,
    @location(1) outline: vec4<f32>,
}

@fragment
fn row(@builtin(position) position: vec4<f32>) -> RowOutput {
    let pixel = vec2<i32>(position.xy);
    var bloom = vec3(0.0);
    var total = 0.0;
    for (var offset = -MAX_BLOOM_RADIUS; offset <= MAX_BLOOM_RADIUS; offset++) {
        let w = weight(offset, reach.x);
        bloom += load(emission, pixel + vec2(offset, 0)).rgb * w;
        total += w;
    }
    let own = load(outline, pixel);
    var found = vec4(0.0, 0.0, 0.0, NONE);
    if own.a > 0.0 {
        found = vec4(own.rgb * 16.0, INSIDE);
    } else {
        var nearest = 1000;
        for (var offset = -MAX_OUTLINE; offset <= MAX_OUTLINE; offset++) {
            let other = load(outline, pixel + vec2(offset, 0));
            let width = round(other.a * 8.0);
            if other.a > 0.0 && f32(abs(offset)) <= width && abs(offset) < nearest {
                nearest = abs(offset);
                found = vec4(other.rgb * 16.0, width * PACK + f32(abs(offset)));
            }
        }
    }
    return RowOutput(vec4(bloom / max(total, 1.0e-6), 1.0), found);
}

@fragment
fn composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    var bloom = vec3(0.0);
    var total = 0.0;
    for (var offset = -MAX_BLOOM_RADIUS; offset <= MAX_BLOOM_RADIUS; offset++) {
        let w = weight(offset, reach.x);
        bloom += load(bloom_row, pixel + vec2(0, offset)).rgb * w;
        total += w;
    }
    bloom /= max(total, 1.0e-6);
    var colour = vec3(0.0);
    var covered = 0.0;
    if load(outline_row, pixel).a != INSIDE {
        var nearest = 1.0e9;
        for (var offset = -MAX_OUTLINE; offset <= MAX_OUTLINE; offset++) {
            let other = load(outline_row, pixel + vec2(0, offset));
            if other.a >= 0.0 {
                let width = floor(other.a / PACK);
                let across = other.a - width * PACK;
                let distance = across * across + f32(offset * offset);
                if distance <= width * width + 0.25 && distance < nearest {
                    nearest = distance;
                    colour = other.rgb;
                    covered = 1.0;
                }
            }
        }
    }
    return vec4(colour * covered + bloom, covered);
}
