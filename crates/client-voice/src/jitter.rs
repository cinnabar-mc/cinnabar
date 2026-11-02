//! Per-speaker jitter buffer: reorders frames by sequence number, drops late ones and
//! tells the caller when a frame is missing so the decoder can conceal the loss.

use std::collections::BTreeMap;

/// What the playout clock should do for the next 20 ms tick.
#[derive(Debug, PartialEq, Eq)]
pub enum Playout<T> {
    /// Decode and play this frame.
    Frame(T),
    /// The frame never arrived; run packet-loss concealment.
    Missing,
    /// Not enough audio buffered yet (or the speaker stopped); play silence.
    Waiting,
}

#[derive(Debug)]
pub struct JitterBuffer<T> {
    frames: BTreeMap<u64, T>,
    next: Option<u64>,
    highest: Option<u64>,
    started: bool,
    target: usize,
    max_depth: usize,
    late: u64,
    lost: u64,
}

impl<T> JitterBuffer<T> {
    /// `target` frames are buffered before playout starts; more than `max_depth` queued
    /// frames makes the buffer skip ahead.
    pub fn new(target: usize, max_depth: usize) -> Self {
        assert!(target >= 1 && max_depth >= target);
        Self {
            frames: BTreeMap::new(),
            next: None,
            highest: None,
            started: false,
            target,
            max_depth,
            late: 0,
            lost: 0,
        }
    }

    pub fn late_frames(&self) -> u64 {
        self.late
    }

    pub fn lost_frames(&self) -> u64 {
        self.lost
    }

    pub fn queued(&self) -> usize {
        self.frames.len()
    }

    /// Unwraps a 32-bit sequence number into a monotonically comparable 64-bit one.
    fn extend(&self, seq: u32) -> u64 {
        match self.highest {
            None => (1u64 << 32) + u64::from(seq),
            Some(h) => {
                let mut d = i64::from(seq) - (h & 0xFFFF_FFFF) as i64;
                if d > 1 << 31 {
                    d -= 1 << 32;
                } else if d < -(1 << 31) {
                    d += 1 << 32;
                }
                (h as i64 + d) as u64
            }
        }
    }

    pub fn push(&mut self, seq: u32, frame: T) {
        let ext = self.extend(seq);
        if self.started && self.next.is_some_and(|n| ext < n) {
            self.late += 1;
            return;
        }
        if self.highest.is_none_or(|h| ext > h) {
            self.highest = Some(ext);
        }
        self.frames.entry(ext).or_insert(frame);
        while self.frames.len() > self.max_depth {
            let first = *self.frames.keys().next().unwrap();
            self.frames.remove(&first);
            self.late += 1;
        }
        // Overflow may have removed the frame we were waiting for.
        if self.started
            && let (Some(n), Some(&first)) = (self.next, self.frames.keys().next())
            && first > n
            && first - n > self.max_depth as u64
        {
            self.next = Some(first);
        }
    }

    /// Called once per playout tick (every 20 ms).
    pub fn pop(&mut self) -> Playout<T> {
        if !self.started {
            if self.frames.len() < self.target {
                return Playout::Waiting;
            }
            self.started = true;
            self.next = self.frames.keys().next().copied();
        }
        let n = self.next.expect("started implies next is set");
        if let Some(frame) = self.frames.remove(&n) {
            self.next = Some(n + 1);
            return Playout::Frame(frame);
        }
        match self.frames.keys().next().copied() {
            None => {
                // Speaker went quiet: rebuffer before the next burst.
                self.started = false;
                self.next = None;
                Playout::Waiting
            }
            Some(first) if first - n > self.max_depth as u64 => {
                self.next = Some(first);
                self.pop()
            }
            Some(_) => {
                self.next = Some(n + 1);
                self.lost += 1;
                Playout::Missing
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(buf: &mut JitterBuffer<u32>, ticks: usize) -> Vec<Playout<u32>> {
        (0..ticks).map(|_| buf.pop()).collect()
    }

    #[test]
    fn waits_for_prebuffer_then_plays_in_order() {
        let mut b = JitterBuffer::new(3, 10);
        b.push(0, 0);
        b.push(1, 1);
        assert_eq!(b.pop(), Playout::Waiting);
        b.push(2, 2);
        assert_eq!(
            drain(&mut b, 3),
            vec![Playout::Frame(0), Playout::Frame(1), Playout::Frame(2)]
        );
        assert_eq!(b.pop(), Playout::Waiting);
    }

    #[test]
    fn reorders_out_of_order_frames() {
        let mut b = JitterBuffer::new(3, 10);
        b.push(2, 2);
        b.push(0, 0);
        b.push(1, 1);
        assert_eq!(
            drain(&mut b, 3),
            vec![Playout::Frame(0), Playout::Frame(1), Playout::Frame(2)]
        );
    }

    #[test]
    fn reports_missing_frames_for_concealment() {
        let mut b = JitterBuffer::new(2, 10);
        b.push(0, 0);
        b.push(2, 2);
        assert_eq!(
            drain(&mut b, 3),
            vec![Playout::Frame(0), Playout::Missing, Playout::Frame(2)]
        );
        assert_eq!(b.lost_frames(), 1);
    }

    #[test]
    fn drops_frames_that_arrive_after_their_slot() {
        let mut b = JitterBuffer::new(2, 10);
        b.push(0, 0);
        b.push(1, 1);
        assert_eq!(b.pop(), Playout::Frame(0));
        assert_eq!(b.pop(), Playout::Frame(1));
        b.push(0, 0);
        assert_eq!(b.late_frames(), 1);
    }

    #[test]
    fn ignores_duplicates() {
        let mut b = JitterBuffer::new(2, 10);
        b.push(0, 10);
        b.push(0, 99);
        b.push(1, 11);
        assert_eq!(b.pop(), Playout::Frame(10));
    }

    #[test]
    fn handles_sequence_wraparound() {
        let mut b = JitterBuffer::new(3, 10);
        b.push(u32::MAX - 1, 0);
        b.push(u32::MAX, 1);
        b.push(0, 2);
        assert_eq!(
            drain(&mut b, 3),
            vec![Playout::Frame(0), Playout::Frame(1), Playout::Frame(2)]
        );
    }

    #[test]
    fn resyncs_after_a_long_pause_instead_of_emitting_endless_gaps() {
        let mut b = JitterBuffer::new(1, 4);
        b.push(0, 0);
        assert_eq!(b.pop(), Playout::Frame(0));
        b.push(500, 500);
        assert_eq!(b.pop(), Playout::Frame(500));
        assert_eq!(b.lost_frames(), 0);
    }

    #[test]
    fn bounds_memory_when_the_consumer_stalls() {
        let mut b = JitterBuffer::new(2, 5);
        for i in 0..50 {
            b.push(i, i);
        }
        assert!(b.queued() <= 5);
    }
}
