//! TEMPORARY DIAGNOSTIC: loops image atomic variants on Metal.

use super::*;

/// TEMPORARY DIAGNOSTIC: counts how often each atomic-counter variant reads a wrong count.
#[test]
fn diag_image_atomics_flake_rate() {
    let Some(gpu) = gpu() else {
        return;
    };
    if !gpu
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ATOMIC)
    {
        return;
    }
    let variants: [(&str, &str); 3] = [
        ("constant coordinate", COUNT_ATOMICALLY),
        (
            "runtime coordinate",
            "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    imageAtomicAdd(counter, ivec2(gl_WorkGroupID.xy), 1u);
}
",
        ),
        (
            "constant coordinate + guarded store",
            "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    imageAtomicAdd(counter, ivec2(0), 1u);
    if (imageSize(counter).x == 0) {
        imageStore(counter, ivec2(0), uvec4(0u));
    }
}
",
        ),
    ];
    let mut report = String::new();
    for (name, compute) in variants {
        let mut wrong = std::collections::BTreeMap::<u8, u32>::new();
        let runs = 120;
        for _ in 0..runs {
            let (_, bytes) = render_frames(
                &gpu,
                &[
                    (
                        "shaders.properties",
                        "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT true false 1 1\n",
                    ),
                    ("composite.csh", compute),
                    ("composite.vsh", PASS_VERTEX),
                    ("composite.fsh", SHOW_COUNT),
                    ("final.vsh", PASS_VERTEX),
                    ("final.fsh", FINAL),
                ],
                2,
            );
            if bytes[0].abs_diff(16) > 1 {
                *wrong.entry(bytes[0]).or_default() += 1;
            }
        }
        report += &format!(
            "{name}: {} / {runs} wrong {wrong:?}\n",
            wrong.values().sum::<u32>()
        );
    }
    panic!("DIAG\n{report}");
}
