//! Builds a pack's pipelines on several threads. A large program's backend compile (DXC and
//! the driver on DX12) takes seconds and blocks only the thread creating its pipeline, so the
//! pack's pipelines compile side by side.

use render_scene::shaderpack::LoadProgress;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Threads building one pack: half the hardware threads, leaving the rest to the running
/// game, and no more than there are pipelines.
fn workers(items: usize) -> usize {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    threads.div_ceil(2).min(items)
}

/// Runs `build` on every item across worker threads and returns the results in item order.
/// Each worker takes the next unbuilt item and captures the out-of-memory errors its device
/// calls raise, which the loader's own scope only sees on its thread. Each finished item counts
/// as built in `progress`. Workers stop taking items once `progress` is cancelled; a build
/// already started finishes first.
pub(super) fn build_all<T: Sync, R: Send>(
    device: &wgpu::Device,
    items: &[T],
    progress: &LoadProgress,
    build: impl Fn(&T) -> R + Sync,
) -> Result<Vec<R>, String> {
    let next = AtomicUsize::new(0);
    let worker = || {
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let mut built = vec![];
        while !progress.is_cancelled() {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(item) = items.get(index) else {
                break;
            };
            built.push((index, build(item)));
            progress.add_built();
        }
        (built, bevy::tasks::block_on(memory.pop()).is_some())
    };
    let finished: Vec<_> = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..workers(items.len()))
            .map(|_| {
                std::thread::Builder::new()
                    .name("shader pack pipelines".into())
                    .spawn_scoped(scope, worker)
                    .map_err(|error| format!("shader pack pipeline thread: {error}"))
            })
            .collect::<Result<_, _>>()?;
        Ok::<_, String>(
            threads
                .into_iter()
                .map(|thread| {
                    thread
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect(),
        )
    })?;
    if finished.iter().any(|(_, out_of_memory)| *out_of_memory) {
        return Err("shader pack load ran out of GPU memory".to_owned());
    }
    if progress.is_cancelled() {
        return Err("shader pack load cancelled".to_owned());
    }
    let mut results: Vec<Option<R>> = items.iter().map(|_| None).collect();
    for (index, result) in finished.into_iter().flat_map(|(built, _)| built) {
        results[index] = Some(result);
    }
    Ok(results
        .into_iter()
        .map(|result| result.expect("every item builds unless cancelled"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn device() -> wgpu::Device {
        wgpu::Device::noop(&wgpu::DeviceDescriptor::default()).0
    }

    #[test]
    fn every_pipeline_builds_once_in_item_order() {
        let built = AtomicUsize::new(0);
        let items: Vec<u32> = (0..40).collect();
        let results = build_all(&device(), &items, &LoadProgress::default(), |item| {
            built.fetch_add(1, Ordering::Relaxed);
            item * 2
        })
        .unwrap();
        assert_eq!(results, items.iter().map(|i| i * 2).collect::<Vec<_>>());
        assert_eq!(built.load(Ordering::Relaxed), items.len());
    }

    /// The loading toast's count reaches every pipeline however the workers share them.
    #[test]
    fn each_built_pipeline_counts_once_toward_the_toast() {
        let progress = LoadProgress::default();
        let items: Vec<u32> = (0..40).collect();
        progress.record_built(3, 3 + items.len() + 1);
        build_all(&device(), &items, &progress, |_| ()).unwrap();
        assert_eq!(progress.pipelines(), Some((43, 44)));
    }

    /// A slow compile does not hold back the next pipeline: the first build only returns once
    /// another one has started beside it.
    #[test]
    fn a_slow_pipeline_builds_beside_the_others() {
        if workers(2) < 2 {
            eprintln!("missing fixture: a host with two hardware threads");
            return;
        }
        let second_started = AtomicBool::new(false);
        let results = build_all(&device(), &[0, 1], &LoadProgress::default(), |&item| {
            if item == 0 {
                test_time::eventually("the second pipeline to start", || {
                    second_started.load(Ordering::Relaxed)
                });
            } else {
                second_started.store(true, Ordering::Relaxed);
            }
            item
        })
        .unwrap();
        assert_eq!(results, [0, 1]);
    }

    /// Switching the pack off stops the load before its remaining pipelines compile: every
    /// worker holds its first pipeline until the load is cancelled, then takes no other.
    #[test]
    fn cancelling_stops_taking_pipelines() {
        let loads =
            std::sync::Mutex::new(render_scene::shaderpack::ShaderPackLoadProgress::default());
        let progress = loads.lock().unwrap().start();
        let built = AtomicUsize::new(0);
        let items: Vec<u32> = (0..64).collect();
        let result = build_all(&device(), &items, &progress, |_| {
            if built.fetch_add(1, Ordering::Relaxed) == 0 {
                loads.lock().unwrap().finish();
            } else {
                test_time::eventually("the load to be cancelled", || progress.is_cancelled());
            }
        });
        assert_eq!(result, Err("shader pack load cancelled".to_owned()));
        assert!(built.load(Ordering::Relaxed) <= workers(items.len()));
    }
}
