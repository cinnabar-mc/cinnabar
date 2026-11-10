//! Image atomics reach the passes after the one that runs them and return the original texel.

use super::*;

/// Counts its 16 invocations into an `r32ui` custom image with an image atomic.
const COUNT_ATOMICALLY: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    imageAtomicAdd(counter, ivec2(0), 1u);
}
";

/// Writes the counter's first texel, over 255, to red.
const SHOW_COUNT: &str = "#version 430
uniform usampler2D counterSampler;
void main() {
/* DRAWBUFFERS:0 */
    uint count = texelFetch(counterSampler, ivec2(0), 0).r;
    gl_FragData[0] = vec4(float(count) / 255.0, 0.0, 0.0, 1.0);
}
";

#[test]
fn image_atomics_count_into_a_custom_image() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    if !gpu
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ATOMIC)
    {
        eprintln!("missing fixture: a GPU with TEXTURE_ATOMIC for image atomics");
        return;
    }
    let (_, bytes) = render_frames(
        &gpu,
        &[
            (
                "shaders.properties",
                "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT true false 1 1\n",
            ),
            ("composite.csh", COUNT_ATOMICALLY),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", SHOW_COUNT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
        2,
    );
    assert_pixels(&bytes, |_| [16, 0, 0]);
}

/// Adds 1 to the counter from each of the 4×4 pixels in the bottom-left corner.
const COUNT_ATOMICALLY_PER_PIXEL: &str = "#version 430
layout(r32ui) uniform uimage2D counter;
void main() {
/* DRAWBUFFERS:1 */
    if (all(lessThan(ivec2(gl_FragCoord.xy), ivec2(4)))) {
        imageAtomicAdd(counter, ivec2(0), 1u);
    }
    gl_FragData[0] = vec4(0.0);
}
";

#[test]
fn fragment_image_atomics_reach_the_next_pass() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    if !gpu
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ATOMIC)
    {
        eprintln!("missing fixture: a GPU with TEXTURE_ATOMIC for image atomics");
        return;
    }
    let (_, bytes) = render_frames(
        &gpu,
        &[
            (
                "shaders.properties",
                "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT true false 1 1\n",
            ),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", COUNT_ATOMICALLY_PER_PIXEL),
            ("composite1.vsh", PASS_VERTEX),
            ("composite1.fsh", SHOW_COUNT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
        2,
    );
    assert_pixels(&bytes, |_| [16, 0, 0]);
}

/// Claims a slot with the counter's original value, then marks that slot in `seen`.
const CLAIM_SLOTS: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
layout(r32ui) uniform uimage2D seen;
void main() {
    uint slot = imageAtomicAdd(counter, ivec2(0), 1u);
    imageAtomicAdd(seen, ivec2(int(slot), 0), 1u);
}
";

/// Writes the number of `seen` slots marked exactly once, over 255, to red.
const SHOW_CLAIMED: &str = "#version 430
uniform usampler2D seenSampler;
void main() {
/* DRAWBUFFERS:0 */
    uint claimed = 0u;
    for (int slot = 0; slot < 16; slot++) {
        claimed += texelFetch(seenSampler, ivec2(slot, 0), 0).r == 1u ? 1u : 0u;
    }
    gl_FragData[0] = vec4(float(claimed) / 255.0, 0.0, 0.0, 1.0);
}
";

#[test]
fn image_atomics_return_the_original_texel() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    if !gpu
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ATOMIC)
    {
        eprintln!("missing fixture: a GPU with TEXTURE_ATOMIC for image atomics");
        return;
    }
    let (_, bytes) = render_frames(
        &gpu,
        &[
            (
                "shaders.properties",
                "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT true false 1 1\n\
                 image.seen = seenSampler RED_INTEGER R32UI UNSIGNED_INT true false 16 1\n",
            ),
            ("composite.csh", CLAIM_SLOTS),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", SHOW_CLAIMED),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
        2,
    );
    assert_pixels(&bytes, |_| [16, 0, 0]);
}
