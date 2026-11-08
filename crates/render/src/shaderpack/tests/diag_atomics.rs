//! TEMPORARY DIAGNOSTIC: loops image atomic variants on Metal.

use super::*;

const STORE_SIXTEEN: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    if (gl_LocalInvocationIndex == 0u) {
        imageStore(counter, ivec2(0), uvec4(16u));
    }
}
";

const CLEARED: &str =
    "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT true false 1 1\n";
const KEPT: &str =
    "image.counter = counterSampler RED_INTEGER R32UI UNSIGNED_INT false false 1 1\n";

/// Reads the counter as a storage image instead of through its sampler.
const SHOW_COUNT_IMAGE: &str = "#version 430
layout(r32ui) uniform uimage2D counter;
void main() {
/* DRAWBUFFERS:0 */
    uint count = imageLoad(counter, ivec2(0)).r;
    gl_FragData[0] = vec4(float(count) / 255.0, 0.0, 0.0, 1.0);
}
";

/// TEMPORARY DIAGNOSTIC: counts how often each counter variant reads a wrong value, with
/// several devices rendering at once as nextest's parallel test processes do.
#[test]
fn diag_image_atomics_flake_rate() {
    let Some(probe) = gpu() else {
        return;
    };
    if !probe
        .device
        .features()
        .contains(wgpu::Features::TEXTURE_ATOMIC)
    {
        return;
    }
    eprintln!("adapter: {:?}", probe.adapter.get_info());
    drop(probe);
    const THREADS: usize = 6;
    const RUNS: usize = 150;
    // (name, properties, compute, fragment, frames, expected red)
    let variants: [(&str, &str, &str, &str, usize, u8); 6] = [
        (
            "A atomic, cleared, 2 frames",
            CLEARED,
            COUNT_ATOMICALLY,
            SHOW_COUNT,
            2,
            16,
        ),
        (
            "B imageStore, cleared, 2 frames",
            CLEARED,
            STORE_SIXTEEN,
            SHOW_COUNT,
            2,
            16,
        ),
        (
            "C atomic, kept, 1 frame",
            KEPT,
            COUNT_ATOMICALLY,
            SHOW_COUNT,
            1,
            16,
        ),
        (
            "D atomic, kept, 2 frames",
            KEPT,
            COUNT_ATOMICALLY,
            SHOW_COUNT,
            2,
            32,
        ),
        (
            "E atomic, cleared, 1 frame",
            CLEARED,
            COUNT_ATOMICALLY,
            SHOW_COUNT,
            1,
            16,
        ),
        (
            "H atomic, imageLoad in fragment, kept, 1 frame",
            KEPT,
            COUNT_ATOMICALLY,
            SHOW_COUNT_IMAGE,
            1,
            16,
        ),
    ];
    let mut report = String::new();
    for (name, properties, compute, fragment, frames, expected) in variants {
        let start = std::time::Instant::now();
        let seen: Vec<std::collections::BTreeMap<u8, u32>> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..THREADS)
                .map(|_| {
                    scope.spawn(move || {
                        let gpu = gpu().expect("adapter");
                        let mut seen = std::collections::BTreeMap::<u8, u32>::new();
                        for _ in 0..RUNS {
                            let (_, bytes) = render_frames(
                                &gpu,
                                &[
                                    ("shaders.properties", properties),
                                    ("composite.csh", compute),
                                    ("composite.vsh", PASS_VERTEX),
                                    ("composite.fsh", fragment),
                                    ("final.vsh", PASS_VERTEX),
                                    ("final.fsh", FINAL),
                                ],
                                frames,
                            );
                            *seen.entry(bytes[0]).or_default() += 1;
                        }
                        seen
                    })
                })
                .collect();
            workers.into_iter().map(|w| w.join().unwrap()).collect()
        });
        let mut total = std::collections::BTreeMap::<u8, u32>::new();
        for map in seen {
            for (value, count) in map {
                *total.entry(value).or_default() += count;
            }
        }
        let wrong: u32 = total
            .iter()
            .filter(|(value, _)| value.abs_diff(expected) > 1)
            .map(|(_, count)| count)
            .sum();
        report += &format!(
            "{name}: {wrong} / {} wrong, expected {expected}, seen {total:?} in {:?}\n",
            THREADS * RUNS,
            start.elapsed()
        );
    }
    panic!("DIAG\n{report}");
}
