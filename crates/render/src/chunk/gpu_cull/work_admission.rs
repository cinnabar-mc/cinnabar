//! Matches captured submitted work to measured costs for an exact render configuration.

use super::model::{CullCamera, CullRecord, DECISION_CANDIDATE, DECISION_HIDDEN, STREAM_COUNT};
use bevy::prelude::Msaa;
use bevy::render::view::ExtractedView;
use serde::Deserialize;

pub(super) const CALIBRATION_ENV: &str = "CINNABAR_TERRAIN_CULL_CALIBRATION";

/// Changing the adapter, backend, viewport or coverage pattern requires another cost measurement.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkContext {
    pub(super) adapter: String,
    pub(super) backend: String,
    pub(super) size: [u32; 2],
    pub(super) samples: u32,
}

impl WorkContext {
    /// The configuration `view` renders with this frame.
    pub(super) fn of(adapter: &wgpu::AdapterInfo, view: &ExtractedView, msaa: Msaa) -> Self {
        Self {
            adapter: adapter.name.clone(),
            backend: format!("{:?}", adapter.backend),
            size: [view.viewport.z, view.viewport.w],
            samples: msaa.samples(),
        }
    }
}

/// Measured cull overhead and terrain cost per submitted index, in stream order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Calibration {
    pub(super) context: WorkContext,
    pub(super) cull_ns: u64,
    pub(super) index_ns: [f64; STREAM_COUNT],
}

impl Calibration {
    /// Rejects incomplete or non-finite measurements instead of inventing a cost.
    pub(super) fn valid(&self) -> bool {
        self.cull_ns > 0
            && self.context.size.into_iter().all(|side| side > 0)
            && self.context.samples.is_power_of_two()
            && self
                .index_ns
                .iter()
                .all(|cost| cost.is_finite() && *cost >= 0.0)
            && self.index_ns.iter().any(|cost| *cost > 0.0)
    }

    /// Old measurements may choose work only for the exact configuration they measured.
    pub(super) fn profitable(
        &self,
        context: &WorkContext,
        hidden: [u64; STREAM_COUNT],
    ) -> Option<bool> {
        (self.valid() && self.context == *context).then(|| {
            hidden
                .into_iter()
                .zip(self.index_ns)
                .map(|(indices, cost)| indices as f64 * cost)
                .sum::<f64>()
                > self.cull_ns as f64
        })
    }
}

/// Borrows the current generation's records in the order used by GPU decision words.
pub(super) struct SubmittedWork<'a> {
    pub(super) records: &'a [CullRecord],
    pub(super) order: &'a [u32],
    pub(super) camera: CullCamera,
    pub(super) index_counts: [u32; STREAM_COUNT],
}

impl SubmittedWork<'_> {
    /// Copies weights while the corresponding decision frame is reserved; slot reuse cannot
    /// substitute a newer generation's ranges when the readback eventually completes.
    pub(super) fn snapshot(&self) -> Vec<[u64; STREAM_COUNT]> {
        self.order
            .iter()
            .map(|&slot| {
                self.records[slot as usize].submitted_indices(self.camera, self.index_counts)
            })
            .collect()
    }
}

/// Totals only captured candidates, keeping stream weights separate for calibration.
pub(super) fn rejected_work(
    decisions: &[u32],
    weights: &[[u64; STREAM_COUNT]],
) -> ([u64; STREAM_COUNT], [u64; STREAM_COUNT]) {
    let (mut candidates, mut hidden) = ([0_u64; STREAM_COUNT], [0_u64; STREAM_COUNT]);
    for (&decision, weights) in decisions.iter().zip(weights) {
        if decision & DECISION_CANDIDATE == 0 {
            continue;
        }
        for stream in 0..STREAM_COUNT {
            candidates[stream] = candidates[stream].saturating_add(weights[stream]);
            if decision & DECISION_HIDDEN != 0 {
                hidden[stream] = hidden[stream].saturating_add(weights[stream]);
            }
        }
    }
    (candidates, hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test calibration with intentionally different costs for the four streams.
    fn calibration() -> Calibration {
        Calibration {
            context: WorkContext {
                adapter: "Test GPU".into(),
                backend: "Vulkan".into(),
                size: [1280, 720],
                samples: 1,
            },
            cull_ns: 100,
            index_ns: [1.0, 2.0, 4.0, 1.0],
        }
    }

    #[test]
    fn hidden_indices_and_stream_cost_decide_profitability_instead_of_section_count() {
        let cost = calibration();
        let (_, hidden) = rejected_work(
            &[DECISION_CANDIDATE | DECISION_HIDDEN, DECISION_CANDIDATE],
            &[[0, 0, 50, 0], [10000, 0, 0, 0]],
        );
        assert_eq!(cost.profitable(&cost.context, hidden), Some(true));
        let (_, hidden) = rejected_work(
            &[DECISION_CANDIDATE | DECISION_HIDDEN; 8],
            &[[1; STREAM_COUNT]; 8],
        );
        assert_eq!(cost.profitable(&cost.context, hidden), Some(false));
        assert_eq!(
            cost.profitable(&cost.context, [100, 0, 0, 0]),
            Some(false),
            "ties cannot repay overhead"
        );
    }

    #[test]
    fn measurements_do_not_cross_adapter_backend_resolution_or_sample_count() {
        let cost = calibration();
        for context in [
            WorkContext {
                adapter: "Other GPU".into(),
                ..cost.context.clone()
            },
            WorkContext {
                backend: "Dx12".into(),
                ..cost.context.clone()
            },
            WorkContext {
                size: [1920, 1080],
                ..cost.context.clone()
            },
            WorkContext {
                samples: 4,
                ..cost.context.clone()
            },
        ] {
            assert_eq!(cost.profitable(&context, [10000; STREAM_COUNT]), None);
        }
    }

    #[test]
    fn disabled_and_out_of_range_decisions_never_add_work() {
        let (candidates, hidden) = rejected_work(
            &[
                DECISION_HIDDEN,
                DECISION_CANDIDATE,
                DECISION_CANDIDATE | DECISION_HIDDEN,
                DECISION_CANDIDATE | DECISION_HIDDEN,
            ],
            &[[100; STREAM_COUNT], [2; STREAM_COUNT], [3; STREAM_COUNT]],
        );
        assert_eq!(candidates, [5; STREAM_COUNT]);
        assert_eq!(hidden, [3; STREAM_COUNT]);
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::super::model::{CullRecordSource, reference_args};
    use super::*;

    #[test]
    fn captured_stream_weights_match_submitted_commands_and_survive_slot_reuse() {
        let camera = CullCamera::new(Some([0.25, 0.25, 0.25]));
        let indices = [6, 6, 12, 6];
        let mut records = [CullRecord::new(&CullRecordSource {
            bounds: [[0; 3], [16; 3]],
            cube: 10..25,
            solid_ends: [2, 4, 6, 8, 10, 12],
            model: 25..29,
            liquid: 29..32,
            ..Default::default()
        })
        .unwrap()];
        let captured = SubmittedWork {
            records: &records,
            order: &[0],
            camera,
            index_counts: indices,
        }
        .snapshot();
        let args = reference_args(&records, &[0], camera, indices, |_| true);
        let expected: [u64; STREAM_COUNT] = std::array::from_fn(|stream| {
            args[stream]
                .iter()
                .map(|draw| u64::from(draw[0]) * u64::from(draw[1]))
                .sum()
        });
        assert_eq!(captured[0], expected);
        records[0] = CullRecord::default();
        assert_eq!(
            rejected_work(&[DECISION_CANDIDATE | DECISION_HIDDEN], &captured).1,
            expected
        );
        assert_eq!(
            records[0].submitted_indices(camera, indices),
            [0; STREAM_COUNT]
        );
    }
}
