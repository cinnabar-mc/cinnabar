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

/// Each invocation exchanges its ticket into the chain texel and marks the ticket it got back,
/// then tries to claim the lock texel with a compare-exchange and records the owner it saw.
const EXCHANGE_TICKETS: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D state;
layout(r32ui) uniform uimage2D seen;
void main() {
    uint ticket = gl_LocalInvocationIndex + 1u;
    uint previous = imageAtomicExchange(state, ivec2(0), ticket);
    imageAtomicAdd(seen, ivec2(int(previous), 0), 1u);
    uint owner = imageAtomicCompSwap(state, ivec2(1, 0), 0u, ticket);
    imageStore(seen, ivec2(int(gl_LocalInvocationIndex), 1), uvec4(owner));
}
";

/// Writes full red when the exchanges returned the starting ticket 0 and 16 distinct tickets, full
/// green when exactly one invocation found the lock free, and full blue when the other 15 saw its
/// final owner.
const SHOW_TICKETS: &str = "#version 430
uniform usampler2D stateSampler;
uniform usampler2D seenSampler;
void main() {
/* DRAWBUFFERS:0 */
    uint unique = 0u;
    for (int ticket = 0; ticket <= 16; ticket++) {
        unique += texelFetch(seenSampler, ivec2(ticket, 0), 0).r == 1u ? 1u : 0u;
    }
    bool startSeen = texelFetch(seenSampler, ivec2(0), 0).r == 1u;
    uint owner = texelFetch(stateSampler, ivec2(1, 0), 0).r;
    uint won = 0u;
    uint lost = 0u;
    for (int invocation = 0; invocation < 16; invocation++) {
        uint seenOwner = texelFetch(seenSampler, ivec2(invocation, 1), 0).r;
        won += seenOwner == 0u ? 1u : 0u;
        lost += seenOwner == owner ? 1u : 0u;
    }
    gl_FragData[0] = vec4(
        startSeen && unique == 16u ? 1.0 : 0.0,
        won == 1u ? 1.0 : 0.0,
        lost == 15u ? 1.0 : 0.0,
        1.0
    );
}
";

#[test]
fn image_exchanges_return_the_original_texel() {
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
                "image.state = stateSampler RED_INTEGER R32UI UNSIGNED_INT true false 2 1\n\
                 image.seen = seenSampler RED_INTEGER R32UI UNSIGNED_INT true false 17 2\n",
            ),
            ("composite.csh", EXCHANGE_TICKETS),
            ("composite.vsh", PASS_VERTEX),
            ("composite.fsh", SHOW_TICKETS),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
        2,
    );
    // The exchanges returned the starting ticket and 16 distinct tickets, one invocation took
    // the free lock, and the other 15 saw its owner without replacing it. Atomics that returned
    // the new texel would miss ticket 0 and leave no invocation seeing a free lock.
    assert_pixels(&bytes, |_| [255, 255, 255]);
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
