//! Nearest-first visiting order of the cull slots, so the compacted draws of every stream reach
//! depth testing front to back.

use bevy::math::Vec3;

use super::model::CullRecord;

const SIDE: i32 = world::SUB_CHUNK_SIDE as i32;
/// Squared sub-chunk distances from here on share one bucket, bounding the counting sort.
const FAR_KEY: u32 = 1 << 16;

/// Record slots in the order the count and emit kernels visit them: by squared distance in
/// sub-chunks from the eye's sub-chunk, nearest first, ties by slot. An eye entering another
/// sub-chunk sorts again. Between crossings new slots join at the back, where newly streamed
/// sections usually lie, reused slots keep their rank and trimmed slots leave. The order decides
/// only how much depth testing rejects, never what draws.
#[derive(Default)]
pub struct DrawOrder {
    /// A permutation of every slot below the record count it was last updated for.
    slots: Vec<u32>,
    eye: Option<[i32; 3]>,
    /// First rank whose slot the GPU copy lacks.
    stale_from: usize,
    keys: Vec<u32>,
    buckets: Vec<u32>,
}

impl DrawOrder {
    #[cfg(test)]
    pub fn slots(&self) -> &[u32] {
        &self.slots
    }

    /// Brings the order up to `records` for an eye in sub-chunk `eye`.
    pub fn update(&mut self, records: &[CullRecord], eye: [i32; 3]) {
        if self.eye != Some(eye) {
            self.eye = Some(eye);
            self.sort(records, eye);
            return;
        }
        let len = records.len();
        if self.slots.len() > len {
            if let Some(first) = self.slots.iter().position(|&slot| slot as usize >= len) {
                self.stale_from = self.stale_from.min(first);
            }
            self.slots.retain(|&slot| (slot as usize) < len);
        }
        // Still a permutation of `0..self.slots.len()`, so exactly the new slots are missing.
        if self.slots.len() < len {
            self.stale_from = self.stale_from.min(self.slots.len());
            self.slots.extend(self.slots.len() as u32..len as u32);
        }
    }

    /// The first rank the GPU copy lacks with the slots from there on, now counted as written.
    pub fn take_stale(&mut self) -> Option<(u32, &[u32])> {
        let first = self.stale_from;
        self.stale_from = self.slots.len();
        (first < self.slots.len()).then(|| (first as u32, &self.slots[first..]))
    }

    /// Rewrites every rank, after the GPU copy was reallocated.
    pub fn mark_stale(&mut self) {
        self.stale_from = 0;
    }

    /// Stable counting sort by [`distance_key`]; dead slots follow every live one.
    fn sort(&mut self, records: &[CullRecord], eye: [i32; 3]) {
        self.keys.clear();
        self.keys
            .extend(records.iter().map(|record| distance_key(record, eye)));
        let dead = self
            .keys
            .iter()
            .copied()
            .filter(|&key| key != u32::MAX)
            .max()
            .map_or(0, |farthest| farthest + 1);
        let bucket = |key: u32| (if key == u32::MAX { dead } else { key }) as usize;
        self.buckets.clear();
        self.buckets.resize(dead as usize + 2, 0);
        for &key in &self.keys {
            self.buckets[bucket(key) + 1] += 1;
        }
        for index in 1..self.buckets.len() {
            self.buckets[index] += self.buckets[index - 1];
        }
        self.slots.clear();
        self.slots.resize(records.len(), 0);
        for (slot, &key) in self.keys.iter().enumerate() {
            let rank = &mut self.buckets[bucket(key)];
            self.slots[*rank as usize] = slot as u32;
            *rank += 1;
        }
        self.stale_from = 0;
    }
}

/// Squared distance in sub-chunks between `record` and sub-chunk `eye`, capped at [`FAR_KEY`];
/// `u32::MAX` for a dead record.
fn distance_key(record: &CullRecord, eye: [i32; 3]) -> u32 {
    if !record.is_live() {
        return u32::MAX;
    }
    let squared: i64 = (0..3)
        .map(|axis| {
            let offset = i64::from(record.origin[axis].div_euclid(SIDE)) - i64::from(eye[axis]);
            offset * offset
        })
        .sum();
    squared.min(i64::from(FAR_KEY)) as u32
}

/// Sub-chunk holding the eye at `position`.
pub fn eye_sub_chunk(position: Vec3) -> [i32; 3] {
    position
        .to_array()
        .map(|value| (value / SIDE as f32).floor() as i32)
}

#[cfg(test)]
mod tests {
    use super::super::model::CullRecordSource;
    use super::*;

    fn record(section: [i32; 3]) -> CullRecord {
        CullRecord::new(&CullRecordSource {
            origin: section.map(|value| value * SIDE),
            cube: 0..1,
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn slots_rank_nearest_first_with_dead_slots_last() {
        let records = [
            record([3, 0, 0]),
            CullRecord::default(),
            record([0, 0, -1]),
            record([-1, 1, 0]),
            record([0, 0, 0]),
            record([0, 0, 1]),
        ];
        let mut order = DrawOrder::default();
        order.update(&records, [0, 0, 0]);
        assert_eq!(order.slots(), [4, 2, 5, 3, 0, 1]);
        assert_eq!(order.take_stale(), Some((0, &[4, 2, 5, 3, 0, 1][..])));
        assert_eq!(order.take_stale(), None);

        order.update(&records, [3, 0, 0]);
        assert_eq!(order.slots(), [0, 4, 2, 5, 3, 1]);
    }

    #[test]
    fn slots_join_at_the_back_and_trimmed_slots_leave_without_a_crossing() {
        let mut records = vec![record([2, 0, 0]), record([1, 0, 0]), record([0, 0, 0])];
        let mut order = DrawOrder::default();
        order.update(&records, [0, 0, 0]);
        order.take_stale();
        records.push(record([0, 1, 0]));
        records.push(record([5, 0, 0]));
        order.update(&records, [0, 0, 0]);
        assert_eq!(order.slots(), [2, 1, 0, 3, 4]);
        assert_eq!(order.take_stale(), Some((3, &[3, 4][..])));

        records.truncate(2);
        order.update(&records, [0, 0, 0]);
        assert_eq!(order.slots(), [1, 0]);
        assert_eq!(order.take_stale(), Some((0, &[1, 0][..])));

        order.mark_stale();
        assert_eq!(order.take_stale(), Some((0, &[1, 0][..])));
    }

    #[test]
    fn eye_sub_chunk_floors_negative_positions() {
        assert_eq!(eye_sub_chunk(Vec3::new(-0.5, 15.9, 16.0)), [-1, 0, 1]);
    }
}
