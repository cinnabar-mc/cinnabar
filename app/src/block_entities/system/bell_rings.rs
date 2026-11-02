//! Bell swings, started once per observed ring cue and forgotten once settled.

use std::sync::Arc;

use bevy::platform::collections::HashMap;
use world::BlockEntityNbt;

/// The `BlockEventPacket` type a bell ring arrives as.
pub(super) const BELL_RING_EVENT_TYPE: i32 = 1;
/// Seconds after a ring when the swing has visibly settled and its state can go.
const BELL_RING_RETAIN_SECONDS: f64 = 10.0;

/// Starts each newly observed ring once and reports its elapsed animation time.
pub(super) fn bell_elapsed(
    rings: &mut HashMap<[i32; 3], (u64, f64)>,
    position: [i32; 3],
    sequence: Option<u64>,
    now: f64,
) -> f32 {
    if let Some(sequence) = sequence {
        let entry = rings.entry(position).or_insert((sequence, now));
        if entry.0 != sequence {
            *entry = (sequence, now);
        }
    }
    rings
        .get(&position)
        .map_or(f32::INFINITY, |(_, start)| (now - start) as f32)
}

/// The last ringing record seen at each bell and when its swing began.
pub(super) type BellDataRings<Record = BlockEntityNbt> = HashMap<[i32; 3], (Arc<Record>, f64)>;

/// Follows the swing a bell's own record describes, which is how a vanilla server rings it.
/// A new record restarts from its tick count, a record that stopped ringing ends the swing,
/// and re-reading the same record carries on.
pub(super) fn bell_data_elapsed<Record>(
    rings: &mut BellDataRings<Record>,
    position: [i32; 3],
    record: &Arc<Record>,
    ring_ticks: Option<u32>,
    now: f64,
) -> f32 {
    let Some(ticks) = ring_ticks else {
        rings.remove(&position);
        return f32::INFINITY;
    };
    let entry = rings
        .entry(position)
        .or_insert_with(|| (Arc::clone(record), now - seconds_of(ticks)));
    if !Arc::ptr_eq(&entry.0, record) {
        *entry = (Arc::clone(record), now - seconds_of(ticks));
    }
    (now - entry.1) as f32
}

/// Seconds a swing has run after `ticks` of the ring's animation count.
fn seconds_of(ticks: u32) -> f64 {
    f64::from(ticks) / f64::from(render_model::BELL_SWING_TICKS_PER_SECOND)
}

/// Forgets data rings that have long settled.
pub(super) fn prune_bell_data_rings<Record>(rings: &mut BellDataRings<Record>, now_seconds: f64) {
    rings.retain(|_, (_, start)| now_seconds - *start < BELL_RING_RETAIN_SECONDS);
}

/// Keeps a latched cue's sequence so an expired swing cannot restart; evicted cues age out.
pub(super) fn prune_bell_rings(
    rings: &mut HashMap<[i32; 3], (u64, f64)>,
    now_seconds: f64,
    mut cue: impl FnMut(&[i32; 3]) -> Option<u64>,
) {
    rings.retain(|position, (sequence, start)| {
        cue(position) == Some(*sequence) || now_seconds - *start < BELL_RING_RETAIN_SECONDS
    });
}
