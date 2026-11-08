//! TEMPORARY DIAGNOSTIC: loops image atomic variants on Metal.

use super::*;

const RUNTIME_COORDINATE: &str = "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {
    imageAtomicAdd(counter, ivec2(gl_WorkGroupID.xy), 1u);
}
";

/// The constant-coordinate counter with a never-taken branch that makes each source unique,
/// so Metal compiles every iteration instead of reusing a cached library.
fn salted(salt: usize) -> String {
    format!(
        "#version 430
layout(local_size_x = 4, local_size_y = 4) in;
const ivec3 workGroups = ivec3(1, 1, 1);
layout(r32ui) uniform uimage2D counter;
void main() {{
    imageAtomicAdd(counter, ivec2(0), 1u);
    if (gl_LocalInvocationIndex == {}u) {{
        imageAtomicAdd(counter, ivec2(0), 1u);
    }}
}}
",
        1000 + salt
    )
}

/// TEMPORARY DIAGNOSTIC: counts how often each atomic-counter variant reads a wrong count,
/// with several devices rendering at once as nextest's parallel test processes do.
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
    drop(probe);
    const THREADS: usize = 6;
    const RUNS: usize = 400;
    let mut report = String::new();
    for variant in [
        "constant coordinate",
        "salted constant coordinate",
        "runtime coordinate",
    ] {
        let start = std::time::Instant::now();
        let wrong: Vec<std::collections::BTreeMap<u8, u32>> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..THREADS)
                .map(|thread| {
                    scope.spawn(move || {
                        let gpu = gpu().expect("adapter");
                        let mut wrong = std::collections::BTreeMap::<u8, u32>::new();
                        for run in 0..RUNS {
                            let salted_source;
                            let compute = match variant {
                                "constant coordinate" => COUNT_ATOMICALLY,
                                "runtime coordinate" => RUNTIME_COORDINATE,
                                _ => {
                                    salted_source = salted(thread * RUNS + run);
                                    &salted_source
                                }
                            };
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
                        wrong
                    })
                })
                .collect();
            workers.into_iter().map(|w| w.join().unwrap()).collect()
        });
        let mut total = std::collections::BTreeMap::<u8, u32>::new();
        for map in wrong {
            for (value, count) in map {
                *total.entry(value).or_default() += count;
            }
        }
        report += &format!(
            "{variant}: {} / {} wrong {total:?} in {:?}\n",
            total.values().sum::<u32>(),
            THREADS * RUNS,
            start.elapsed()
        );
    }
    panic!("DIAG\n{report}");
}
