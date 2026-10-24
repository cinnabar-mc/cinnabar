// HDR output stages over linear BT.709 colour where 1.0 is paper white.

@group(0) @binding(0) var source: texture_2d<f32>;

// Mirrors `HdrStageConstants` in hdr_output.rs.
struct HdrOutput {
    paper_white_nits: f32,
    // The level SDR white expands to; 1.0 keeps the scene unchanged.
    sdr_white: f32,
    // Luminance where expansion begins.
    knee: f32,
    padding: f32,
}
@group(0) @binding(1) var<uniform> hdr: HdrOutput;

/// Covers the target with one triangle.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

/// Lifts SDR highlights toward `sdr_white`: colour below the knee is unchanged and the
/// brightest channel rises along a curve whose slope matches 1 at the knee, so hue is kept.
fn expand(colour: vec3<f32>) -> vec3<f32> {
    let peak = max(colour.r, max(colour.g, colour.b));
    if hdr.sdr_white <= 1.0 || peak <= hdr.knee {
        return colour;
    }
    let t = min((peak - hdr.knee) / (1.0 - hdr.knee), 1.0);
    let expanded = peak + (hdr.sdr_white - 1.0) * t * t;
    return colour * (expanded / peak);
}

/// Copies a same-sized scene into the HDR UI target, expanding SDR highlights.
@fragment
fn expand_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = textureLoad(source, vec2<i32>(position.xy), 0);
    return vec4<f32>(expand(max(texel.rgb, vec3<f32>(0.0))), texel.a);
}

/// BT.709 to BT.2020 primaries (ITU-R BT.2087).
const BT709_TO_BT2020 = mat3x3<f32>(
    vec3<f32>(0.627404, 0.069097, 0.016391),
    vec3<f32>(0.329283, 0.919540, 0.088013),
    vec3<f32>(0.043313, 0.011362, 0.895595),
);

/// SMPTE ST 2084 inverse EOTF of luminance relative to 10000 nits.
fn pq(luminance: vec3<f32>) -> vec3<f32> {
    let m1 = 2610.0 / 16384.0;
    let m2 = 2523.0 / 4096.0 * 128.0;
    let c1 = 3424.0 / 4096.0;
    let c2 = 2413.0 / 4096.0 * 32.0;
    let c3 = 2392.0 / 4096.0 * 32.0;
    let power = pow(clamp(luminance, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(m1));
    return pow((c1 + c2 * power) / (1.0 + c3 * power), vec3<f32>(m2));
}

/// Writes the composed frame as scRGB: linear BT.709 where 1.0 is 80 nits.
@fragment
fn encode_scrgb_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = textureLoad(source, vec2<i32>(position.xy), 0);
    return vec4<f32>(texel.rgb * (hdr.paper_white_nits / 80.0), 1.0);
}

/// Writes the composed frame as HDR10: BT.2020 primaries with the PQ transfer.
@fragment
fn encode_pq_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = textureLoad(source, vec2<i32>(position.xy), 0);
    let bt2020 = max(BT709_TO_BT2020 * texel.rgb, vec3<f32>(0.0));
    return vec4<f32>(pq(bt2020 * hdr.paper_white_nits / 10000.0), 1.0);
}
