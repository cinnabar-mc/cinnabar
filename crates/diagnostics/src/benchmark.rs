//! Exact frame statistics for opt-in benchmark reports, independent of rolling diagnostics.

use serde::{Deserialize, Serialize};

/// Version-one lower histogram edges in milliseconds; the final bucket has no upper bound.
pub const HISTOGRAM_EDGES_MS: [f64; 24] = [
    0., 1., 2., 3., 4.17, 5., 6., 7., 8.33, 10., 12., 14., 16.67, 20., 25., 33.33, 40., 50., 66.67,
    100., 150., 250., 500., 1000.,
];

/// One independently measured value. A single run has unknown variance and confidence interval.
#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub stdev: Option<f64>,
    pub min: f64,
    pub max: f64,
    pub ci95: Option<f64>,
}

impl Summary {
    /// Wraps one finite run-level scalar without treating adjacent frames as independent runs.
    pub fn single(value: f64) -> Result<Self, String> {
        if !value.is_finite() {
            return Err("nonfinite benchmark scalar".into());
        }
        Ok(Self {
            n: 1,
            mean: round(value),
            stdev: None,
            min: round(value),
            max: round(value),
            ci95: None,
        })
    }
}

/// Exact interval statistics for one phase of one run. CPU and GPU semantics remain separate.
#[derive(Debug, Serialize, Deserialize)]
pub struct Frames {
    pub phase: String,
    pub repeat: u32,
    pub frames: usize,
    pub seconds: f64,
    pub avg_fps: f64,
    pub low_1pct_fps: f64,
    pub low_01pct_fps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub max_ms: f64,
    pub histogram: Vec<u64>,
}

impl Frames {
    /// Rejects missing or invalid intervals, preserving full stalls and exact slow-tail means.
    pub fn new(phase: &str, intervals: &[f64]) -> Result<Self, String> {
        let values = sorted(intervals, false)?;
        let n = values.len();
        let total: f64 = values.iter().sum();
        if !total.is_finite() {
            return Err("benchmark interval sum overflowed".into());
        }
        let low = |fraction: f64| {
            let count = (n as f64 * fraction).ceil() as usize;
            1000. * count as f64 / values[n - count..].iter().sum::<f64>()
        };
        let mut histogram = vec![0; HISTOGRAM_EDGES_MS.len()];
        for value in &values {
            histogram[HISTOGRAM_EDGES_MS.partition_point(|edge| edge <= value) - 1] += 1;
        }
        Ok(Self {
            phase: phase.into(),
            repeat: 0,
            frames: n,
            seconds: round(total / 1000.),
            avg_fps: round(1000. * n as f64 / total),
            low_1pct_fps: round(low(0.01)),
            low_01pct_fps: round(low(0.001)),
            p50_ms: round(rank(&values, 0.5)),
            p95_ms: round(rank(&values, 0.95)),
            p99_ms: round(rank(&values, 0.99)),
            p999_ms: round(rank(&values, 0.999)),
            max_ms: round(values[n - 1]),
            histogram,
        })
    }
}

/// Native stage wall durations. Overlapping stages must never be added into a frame cost.
#[derive(Debug, Serialize)]
pub struct Distribution {
    pub samples: usize,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

impl Distribution {
    /// Allows zero-duration stage samples, but never missing or nonfinite observations.
    pub fn new(samples: &[f64]) -> Result<Self, String> {
        let values = sorted(samples, true)?;
        let mean_ms = values
            .iter()
            .map(|value| value / values.len() as f64)
            .sum::<f64>();
        if !mean_ms.is_finite() {
            return Err("benchmark stage mean overflowed".into());
        }
        Ok(Self {
            samples: values.len(),
            mean_ms: round(mean_ms),
            p50_ms: round(rank(&values, 0.5)),
            p95_ms: round(rank(&values, 0.95)),
            p99_ms: round(rank(&values, 0.99)),
            max_ms: round(values[values.len() - 1]),
        })
    }
}

/// Validates the complete sample before sorting; invalid values are never silently filtered.
fn sorted(samples: &[f64], allow_zero: bool) -> Result<Vec<f64>, String> {
    if samples.is_empty()
        || samples
            .iter()
            .any(|v| !v.is_finite() || *v < 0. || !allow_zero && *v == 0.)
    {
        return Err("benchmark samples are missing, nonfinite or outside their domain".into());
    }
    let mut values = samples.to_vec();
    values.sort_by(f64::total_cmp);
    Ok(values)
}

/// Selects the nearest rank of a validated, nonempty sorted sample.
fn rank(values: &[f64], fraction: f64) -> f64 {
    values[(fraction * values.len() as f64).ceil() as usize - 1]
}

/// Rounds exported values to three decimals, with ties away from zero.
fn round(value: f64) -> f64 {
    (value * 1000.).round() / 1000.
}

#[cfg(test)]
mod tests;
