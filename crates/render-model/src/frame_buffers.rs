//! Reuses plain frame arrays after every immutable reader releases them.

use std::sync::Arc;

mod scratch;
pub use scratch::{CapacityRetention, FrameScratch};

/// Retained allocations are a cache; readers beyond it always receive fresh storage.
const CACHED_BUFFERS: usize = 4;

/// Producer-owned scratch for immutable frame arrays, including vertex publications.
#[derive(Debug)]
pub struct FrameBuffers<T, const CAPACITY: usize = CACHED_BUFFERS> {
    retired: Vec<Arc<[T]>>,
    retention: CapacityRetention,
}

impl<T, const CAPACITY: usize> Default for FrameBuffers<T, CAPACITY> {
    /// Starts without allocating storage; `CAPACITY` must be positive.
    fn default() -> Self {
        assert!(CAPACITY > 0, "frame storage needs a positive cache budget");
        Self {
            retired: Vec::new(),
            retention: CapacityRetention::default(),
        }
    }
}

impl<T, const CAPACITY: usize> Clone for FrameBuffers<T, CAPACITY> {
    /// Copies of published frames do not retain the producer's reusable allocations.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl<T, const CAPACITY: usize> FrameBuffers<T, CAPACITY> {
    /// Releases cached peaks after sustained lower use, including unchanged publications.
    pub fn trim_for_len(&mut self, len: usize) {
        let high = self
            .retired
            .iter()
            .map(|buffer| buffer.len())
            .max()
            .unwrap_or(0);
        let target = self.retention.target(high, len, 0);
        if target < high {
            self.retired.retain(|buffer| buffer.len() <= target);
        }
    }
}

impl<T: Clone, const CAPACITY: usize> FrameBuffers<T, CAPACITY> {
    /// Copies and edits a publication using only storage that no reader still holds.
    /// Oversized retired arrays are released after a sustained smaller workload.
    pub fn publish_with(&mut self, source: &[T], edit: impl FnOnce(&mut [T])) -> Arc<[T]> {
        self.trim_for_len(source.len());
        let index = self
            .retired
            .iter_mut()
            .position(|buffer| buffer.len() == source.len() && Arc::get_mut(buffer).is_some());
        let index = if let Some(index) = index {
            Arc::get_mut(&mut self.retired[index])
                .expect("exclusive frame storage")
                .clone_from_slice(source);
            index
        } else {
            if self.retired.len() == CAPACITY {
                self.retired.remove(0);
            }
            self.retired.push(Arc::from(source));
            self.retired.len() - 1
        };
        let buffer = &mut self.retired[index];
        edit(Arc::get_mut(buffer).expect("exclusive frame storage"));
        Arc::clone(buffer)
    }
}

impl<T: Copy + bytemuck::Zeroable, const CAPACITY: usize> FrameBuffers<T, CAPACITY> {
    /// Fills an exclusive array and publishes it immutably. `fill` must write every element;
    /// an error publishes nothing and leaves all existing readers unchanged.
    pub fn write<E>(
        &mut self,
        len: usize,
        fill: impl FnOnce(&mut [T]) -> Result<(), E>,
    ) -> Result<Arc<[T]>, E> {
        let index = self
            .retired
            .iter_mut()
            .position(|buffer| buffer.len() == len && Arc::get_mut(buffer).is_some());
        let index = index.unwrap_or_else(|| {
            if self.retired.len() == CAPACITY {
                self.retired.remove(0);
            }
            self.retired.push(vec![T::zeroed(); len].into());
            self.retired.len() - 1
        });
        let buffer = &mut self.retired[index];
        fill(Arc::get_mut(buffer).expect("exclusive frame storage"))?;
        Ok(Arc::clone(buffer))
    }
}

#[cfg(test)]
mod retention_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_publications_reuse_storage_without_changing_retained_readers() {
        let mut pool = FrameBuffers::<u32>::default();
        let mut current = Arc::from([]);
        for value in 0..CACHED_BUFFERS + 2 {
            current = pool
                .write(64, |output| {
                    output.fill(value as u32);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        let held = current;
        let weak = Arc::downgrade(&held);
        let (next, allocations) = test_alloc::count(|| {
            pool.write(64, |output| {
                output.fill(99);
                Ok::<_, ()>(())
            })
            .unwrap()
        });
        assert_eq!(allocations, 0);
        assert!(
            held.iter()
                .all(|value| *value == (CACHED_BUFFERS + 1) as u32)
        );
        assert!(next.iter().all(|value| *value == 99));
        assert!(Arc::ptr_eq(&weak.upgrade().unwrap(), &held));
    }

    #[test]
    fn readers_beyond_the_cache_keep_every_frame_and_failed_fills_publish_nothing() {
        let mut pool = FrameBuffers::<u32>::default();
        let held: Vec<_> = (0..CACHED_BUFFERS + 3)
            .map(|value| {
                pool.write(8, |output| {
                    output.fill(value as u32);
                    Ok::<_, ()>(())
                })
                .unwrap()
            })
            .collect();
        assert_eq!(
            pool.write(8, |output| {
                output.fill(999);
                Err::<(), _>("rejected")
            }),
            Err("rejected")
        );
        for (value, frame) in held.iter().enumerate() {
            assert!(frame.iter().all(|entry| *entry == value as u32));
        }
        drop(held);
        let (frame, allocations) = test_alloc::count(|| {
            pool.write(8, |output| {
                output.fill(7);
                Ok::<_, ()>(())
            })
            .unwrap()
        });
        assert_eq!(allocations, 0);
        assert!(frame.iter().all(|value| *value == 7));
    }
}
