//! Reduces existing stage traces without changing their clocks or GPU attribution.

use super::Route;
use diagnostics::benchmark::{Distribution, Frames, Summary};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Reduced {
    pub frames: Vec<Frames>,
    pub metrics: BTreeMap<String, Summary>,
    pub phases: Vec<Value>,
}

/// Reads one required numeric observation, rejecting null, nonfinite and wrongly typed data.
fn number(value: &Value, key: &str) -> Result<f64, String> {
    value[key]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("missing trace {key}"))
}

/// Requires retained frame sequences and successor starts, including stalls beyond a route's end.
pub(super) fn reduce(trace: &Value, routes: &[Route], warmup: f32) -> Result<Reduced, String> {
    if trace["truncated"] != false
        || trace["dropped_events"] != 0
        || trace["overwritten_events"] != 0
    {
        return Err("stage trace is incomplete or has overwritten events".into());
    }
    let events = trace["traceEvents"]
        .as_array()
        .ok_or("trace has no events")?;
    let starts: Vec<_> = events
        .iter()
        .filter(|e| e["name"] == "frame_start")
        .map(|e| {
            Ok((
                number(e, "ts")?,
                e["frame_id"].as_u64().ok_or("missing frame ID")?,
            ))
        })
        .collect::<Result<_, String>>()?;
    if starts
        .windows(2)
        .any(|p| p[1].0 <= p[0].0 || p[1].1 != p[0].1 + 1)
    {
        return Err("frame starts are nonmonotonic or have missing sequences".into());
    }
    let marks: Vec<_> = events
        .iter()
        .filter(|e| e["name"] == "benchmark_path_start")
        .map(|e| number(e, "ts"))
        .collect::<Result<_, _>>()?;
    if marks.len() != routes.len() {
        return Err("route markers are missing or duplicated".into());
    }
    let mut result = Reduced {
        frames: Vec::new(),
        metrics: BTreeMap::new(),
        phases: Vec::new(),
    };
    for (route, origin) in routes.iter().zip(marks) {
        let begin = origin + f64::from(warmup) * 1e6;
        let end = origin + f64::from(route.path(warmup)?.duration()) * 1e6;
        let selected: Vec<_> = starts
            .windows(2)
            .filter(|p| p[0].0 >= begin && p[0].0 < end)
            .collect();
        let last = selected
            .last()
            .ok_or("flight has no complete frame intervals")?;
        if last[1].0 < end {
            return Err("flight has no successor start closing its final interval".into());
        }
        let ids: BTreeSet<_> = selected.iter().map(|p| p[0].1).collect();
        let intervals: Vec<_> = selected.iter().map(|p| (p[1].0 - p[0].0) / 1000.).collect();
        let frames = Frames::new(&route.name, &intervals)?;
        let mut native: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut gpu: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for event in events {
            let name = event["name"].as_str().ok_or("trace event missing name")?;
            if event["ph"] == "X"
                && event["frame_id"]
                    .as_u64()
                    .is_some_and(|id| ids.contains(&id))
                && name != "benchmark_path_start"
            {
                native
                    .entry(name.into())
                    .or_default()
                    .push(number(event, "dur")? / 1000.);
            }
            if event["cat"] == "gpu_readback"
                && number(event, "ts")? >= begin
                && number(event, "ts")? < end
            {
                gpu.entry(name.into())
                    .or_default()
                    .push(number(&event["args"], "duration_ns")? / 1e6);
            }
        }
        let cpu = distributions(native)?;
        let gpu = distributions(gpu)?;
        for (kind, stages) in [("cpu_spans", &cpu), ("gpu_received", &gpu)] {
            for (stage, distribution) in stages {
                for (stat, value) in [
                    ("mean_ms", distribution.mean_ms),
                    ("p50_ms", distribution.p50_ms),
                    ("p95_ms", distribution.p95_ms),
                    ("p99_ms", distribution.p99_ms),
                    ("max_ms", distribution.max_ms),
                ] {
                    result.metrics.insert(
                        format!("{kind}.{}.{stage}.{stat}", route.name),
                        Summary::single(value)?,
                    );
                }
            }
        }
        // Each scalar remains one run-level observation; frames are not independent repeats.
        for (name, value) in [
            ("avg_fps", frames.avg_fps),
            ("low_1pct_fps", frames.low_1pct_fps),
            ("low_01pct_fps", frames.low_01pct_fps),
        ] {
            result.metrics.insert(
                format!("fps.{}.{name}", route.name),
                Summary::single(value)?,
            );
        }
        result.phases.push(json!({
            "name": route.name, "first_frame_id": selected[0][0].1, "last_frame_id": last[0].1,
            "intended_seconds": (end - begin) / 1e6,
            "interval_seconds": intervals.iter().sum::<f64>() / 1000.,
            "overrun_seconds": (last[1].0 - end) / 1e6,
            "cpu_stage_spans": cpu, "gpu_received_samples": gpu,
            "gpu_origin_ids": "unavailable", "gpu_tail_coverage": "unknown",
        }));
        result.frames.push(frames);
    }
    Ok(result)
}

/// Keeps each native stage separate; overlapping CPU/GPU spans are never added.
fn distributions(
    values: BTreeMap<String, Vec<f64>>,
) -> Result<BTreeMap<String, Distribution>, String> {
    values
        .into_iter()
        .map(|(name, values)| Ok((name, Distribution::new(&values)?)))
        .collect()
}
