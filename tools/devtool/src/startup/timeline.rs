//! Partitions launch-to-menu time into critical-path segments and summarizes pre-menu work.

use super::{Distribution, distribution};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The interval between two consecutive milestones of one launch.
#[derive(Serialize, Deserialize)]
pub(super) struct Segment {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) samples: usize,
    pub(super) start: Distribution,
    pub(super) duration: Distribution,
}

/// Wall latency of one pipeline from first sighting to its first usable frame.
#[derive(Serialize, Deserialize)]
pub(super) struct PipelineLatency {
    /// Launches in which the pipeline was not yet usable when the menu was presented.
    pub(super) unready: usize,
    pub(super) ready: Option<Distribution>,
    pub(super) latency: Option<Distribution>,
}

/// Returns one launch's segments in time order; their durations sum to its `menu_ms`.
fn segments(sample: &Value) -> Vec<(String, String, f64, f64)> {
    let mut marks: Vec<(String, f64)> = sample["milestones_ms"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, at)| Some((name.clone(), at.as_f64()?)))
        .collect();
    marks.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let mut previous = ("launch".to_owned(), 0.0);
    let mut result = Vec::with_capacity(marks.len());
    for (name, at) in marks {
        result.push((
            previous.0.clone(),
            name.clone(),
            previous.1,
            at - previous.1,
        ));
        previous = (name, at);
    }
    result
}

/// Summarizes segments by their bounding milestones and orders them by median start.
pub(super) fn summarize(samples: &[Value]) -> Vec<Segment> {
    let mut grouped: BTreeMap<(String, String), (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for sample in samples {
        for (from, to, start, duration) in segments(sample) {
            let entry = grouped.entry((from, to)).or_default();
            entry.0.push(start);
            entry.1.push(duration);
        }
    }
    let mut result: Vec<Segment> = grouped
        .into_iter()
        .map(|((from, to), (starts, durations))| Segment {
            from,
            to,
            samples: durations.len(),
            start: distribution(starts),
            duration: distribution(durations),
        })
        .collect();
    result.sort_by(|a, b| a.start.median_ms.total_cmp(&b.start.median_ms));
    result
}

/// Groups pipelines by label and occurrence, so repeated labels stay distinguishable.
pub(super) fn pipelines(samples: &[Value]) -> BTreeMap<String, PipelineLatency> {
    let mut grouped: BTreeMap<String, (usize, Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for sample in samples {
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for pipeline in sample["pipelines"].as_array().into_iter().flatten() {
            let label = pipeline["label"].as_str().unwrap_or("unlabeled");
            let occurrence = seen.entry(label).or_default();
            *occurrence += 1;
            let entry = grouped.entry(format!("{label}#{occurrence}")).or_default();
            match (
                pipeline["queued_ms"].as_f64(),
                pipeline["ready_ms"].as_f64(),
            ) {
                (Some(queued), Some(ready)) => {
                    entry.1.push(ready);
                    entry.2.push(ready - queued);
                }
                _ => entry.0 += 1,
            }
        }
    }
    grouped
        .into_iter()
        .map(|(name, (unready, ready, latency))| {
            let ready = (!ready.is_empty()).then(|| distribution(ready));
            let latency = (!latency.is_empty()).then(|| distribution(latency));
            (
                name,
                PipelineLatency {
                    unready,
                    ready,
                    latency,
                },
            )
        })
        .collect()
}

/// Reports each work counter's largest value across launches; work never varies by timing alone.
pub(super) fn work(samples: &[Value]) -> BTreeMap<String, u64> {
    let mut result = BTreeMap::new();
    for sample in samples {
        for (name, value) in sample["counters"].as_object().into_iter().flatten() {
            if let Some(value) = value.as_u64() {
                let entry = result.entry(name.clone()).or_default();
                *entry = value.max(*entry);
            }
        }
        if let Some(count) = sample["pipeline_count"].as_u64() {
            let entry = result.entry("pipelines".to_owned()).or_default();
            *entry = count.max(*entry);
        }
    }
    result
}

/// Lists work counters that grew beyond a saved run, which timing noise cannot explain.
pub(super) fn work_growth(
    current: &BTreeMap<String, u64>,
    baseline: &BTreeMap<String, u64>,
) -> Vec<String> {
    current
        .iter()
        .filter_map(|(name, &value)| {
            let previous = baseline.get(name).copied().unwrap_or(0);
            (value > previous).then(|| format!("{name} {previous} -> {value}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_partition_the_menu_time_whatever_order_milestones_arrive_in() {
        let sample = serde_json::json!({
            "menu_ms": 10.0,
            "milestones_ms": {"menu_presented": 10.0, "carriers": 2.0, "first_update_start": 6.0},
        });
        let parts = segments(&sample);
        let names: Vec<_> = parts
            .iter()
            .map(|(a, b, _, _)| format!("{a}>{b}"))
            .collect();
        assert_eq!(
            names,
            [
                "launch>carriers",
                "carriers>first_update_start",
                "first_update_start>menu_presented"
            ]
        );
        assert_eq!(parts.iter().map(|part| part.3).sum::<f64>(), 10.0);
    }

    #[test]
    fn repeated_pipeline_labels_and_unready_pipelines_stay_separate() {
        let sample = serde_json::json!({"pipelines": [
            {"label": "ui", "queued_ms": 1.0, "ready_ms": 5.0},
            {"label": "ui", "queued_ms": 2.0, "ready_ms": 12.0},
            {"label": "blit", "queued_ms": 2.0, "ready_ms": null},
        ]});
        let summary = pipelines(&[sample]);
        assert_eq!(summary["ui#1"].latency.as_ref().unwrap().median_ms, 4.0);
        assert_eq!(summary["ui#2"].latency.as_ref().unwrap().median_ms, 10.0);
        assert_eq!(summary["blit#1"].unready, 1);
        assert!(summary["blit#1"].ready.is_none());
    }

    #[test]
    fn work_growth_names_counters_that_exceed_the_baseline() {
        let before = work(&[serde_json::json!({"counters": {"uploads": 4}, "pipeline_count": 7})]);
        let after = work(&[
            serde_json::json!({"counters": {"uploads": 4, "modules": 1}, "pipeline_count": 9}),
            serde_json::json!({"counters": {"uploads": 3}, "pipeline_count": 7}),
        ]);
        assert_eq!(
            work_growth(&after, &before),
            ["modules 0 -> 1", "pipelines 7 -> 9"]
        );
    }
}
