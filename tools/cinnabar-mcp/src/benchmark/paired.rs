//! Alternating paired runs of a base and a candidate build under matched conditions.

use super::{files, report, run};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Fewer pairs give no meaningful spread.
const MIN_ROUNDS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Base,
    Candidate,
}

impl Side {
    fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Candidate => "candidate",
        }
    }
}

/// Spread of one metric's candidate/base ratio across rounds.
#[derive(Debug, PartialEq, Serialize)]
struct Paired {
    rounds: usize,
    median: f64,
    p25: f64,
    p75: f64,
}

/// Alternates the run order each round, so drift such as heating penalises both builds alike.
fn order(round: usize) -> [Side; 2] {
    if round.is_multiple_of(2) {
        [Side::Base, Side::Candidate]
    } else {
        [Side::Candidate, Side::Base]
    }
}

/// Runs `rounds` pairs, then writes and prints `paired.json` and records the attempt's outcome.
/// Fails when the candidate's average FPS is lower in at least three quarters of the pairs for
/// any phase.
pub(super) fn run(
    repo: &Path,
    base: &Path,
    candidate: &Path,
    rounds: usize,
    output: &Path,
) -> Result<(), String> {
    if rounds < MIN_ROUNDS {
        return Err(format!(
            "paired benchmarks need at least {MIN_ROUNDS} rounds"
        ));
    }
    let output = run::new_output_directory(repo, output)?;
    let result = compare_rounds(repo, base, candidate, rounds, &output);
    let outcome = match &result {
        Ok(()) => serde_json::json!({"status": "complete"}),
        Err(error) => serde_json::json!({"status": "failed_or_rejected", "error": error}),
    };
    files::write(&output.join("outcome.json"), &outcome)?;
    result
}

/// Runs and validates each pair as it completes, then summarizes every round.
#[expect(
    clippy::print_stdout,
    reason = "the explicit comparison CLI returns its JSON result on stdout"
)]
fn compare_rounds(
    repo: &Path,
    base: &Path,
    candidate: &Path,
    rounds: usize,
    output: &Path,
) -> Result<(), String> {
    let run_dir =
        |round: usize, side: Side| output.join(format!("round-{round:02}-{}", side.name()));
    let inputs = [runtime_inputs(base)?, runtime_inputs(candidate)?];
    if inputs[0] != inputs[1] {
        return Err(format!(
            "the builds load different cores or carriers, so FPS would not compare the builds alone: {inputs:?}"
        ));
    }
    files::write(&output.join("runtime-inputs.json"), &inputs[0])?;
    let mut pairs = Vec::with_capacity(rounds);
    let mut observed = BTreeSet::new();
    let mut first: Option<[Value; 3]> = None;
    for round in 0..rounds {
        for side in order(round) {
            let plan = match side {
                Side::Base => base,
                Side::Candidate => candidate,
            };
            run::run(repo, plan, &run_dir(round, side))?;
        }
        // Stop as soon as a pair cannot join the comparison, before spending more runs.
        let (b, bm) = report::load(&run_dir(round, Side::Base).join("report.json"))?;
        let (c, cm) = report::load(&run_dir(round, Side::Candidate).join("report.json"))?;
        report::compatible(&c, &cm, &b, &bm)?;
        let inputs = [
            bm["comparison_key"].clone(),
            bm["hashes"]["binary"].clone(),
            cm["hashes"]["binary"].clone(),
        ];
        if *first.get_or_insert_with(|| inputs.clone()) != inputs {
            return Err(format!(
                "round {round} measured a different build or workload than round 0"
            ));
        }
        let (b, c) = (metric_values(&b)?, metric_values(&c)?);
        observed.extend(b.keys().chain(c.keys()).cloned());
        pairs.push(ratios(&b, &c));
    }
    let summary = summarize(&pairs);
    let unavailable = unpaired(&observed, &pairs);
    let slower = mostly_slower(&pairs);
    let result = serde_json::json!({
        "comparison": "alternating_pairs", "candidate_over_base": summary,
        "slower_average_fps": slower, "qualification": "incomplete",
        "gpu_comparison": "unsupported_without_origin_ids",
        "unavailable_at_report_precision": unavailable,
    });
    files::write(&output.join("paired.json"), &result)?;
    println!("{result}");
    if slower.is_empty() {
        Ok(())
    } else {
        Err(format!("the candidate is slower in {slower:?}"))
    }
}

/// Hashes of the Go core and compiled carriers the plan's client resolves beside its binary.
fn runtime_inputs(plan_file: &Path) -> Result<Value, String> {
    let plan: super::Plan = files::read(plan_file)?;
    let binary = plan_file
        .parent()
        .ok_or("plan has no parent directory")?
        .join(&plan.binary)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let layout = launcher::install_layout::InstallLayout::discover_for(binary)
        .map_err(|e| format!("resolve the client's install layout: {e}"))?;
    Ok(serde_json::json!({
        "core_sha256": files::hash_file(&layout.core_executable)?,
        "carriers_sha256": files::hash_tree(&layout.compiled_assets)?,
    }))
}

/// Metrics either build reported that some round could not pair: missing from one build, or a
/// stage mean that rounds to zero at the reports' millisecond precision. Summarizing only the
/// other rounds would bias their spread, and dropping them would hide a removed stage.
fn unpaired(observed: &BTreeSet<String>, pairs: &[BTreeMap<String, f64>]) -> Vec<String> {
    observed
        .iter()
        .filter(|key| pairs.iter().filter(|pair| pair.contains_key(*key)).count() != pairs.len())
        .cloned()
        .collect()
}

/// Average-FPS metrics whose candidate/base ratio is below one in at least three quarters of the
/// pairs that measured them.
fn mostly_slower(pairs: &[BTreeMap<String, f64>]) -> Vec<String> {
    let mut counts: BTreeMap<&String, (usize, usize)> = BTreeMap::new();
    for pair in pairs {
        for (key, ratio) in pair.iter().filter(|(key, _)| key.ends_with(".avg_fps")) {
            let (slower, measured) = counts.entry(key).or_default();
            *slower += usize::from(*ratio < 1.);
            *measured += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, (slower, measured))| 4 * slower >= 3 * measured)
        .map(|(key, _)| key.clone())
        .collect()
}

/// CPU stage means are the only run metrics paired; GPU samples lack source-frame identity, so
/// their tails can move between routes without any change in execution time.
fn cpu_stage_mean(name: &str) -> bool {
    name.starts_with("cpu_spans.") && name.ends_with(".mean_ms")
}

/// Every frame statistic and CPU stage mean a report carries.
fn metric_values(report: &Value) -> Result<BTreeMap<String, f64>, String> {
    let mut values = BTreeMap::new();
    for phase in report["frames"].as_array().ok_or("missing frames")? {
        let name = phase["phase"].as_str().ok_or("missing phase name")?;
        for metric in report::FRAME_METRICS {
            if let Some(value) = phase[metric].as_f64() {
                values.insert(format!("frames.{name}.{metric}"), value);
            }
        }
    }
    if let Some(metrics) = report["metrics"].as_object() {
        for (name, summary) in metrics {
            if let Some(mean) = summary["mean"].as_f64().filter(|_| cpu_stage_mean(name)) {
                values.insert(name.clone(), mean);
            }
        }
    }
    Ok(values)
}

/// Candidate/base ratios of the metrics both builds report with positive, finite values.
fn ratios(
    base: &BTreeMap<String, f64>,
    candidate: &BTreeMap<String, f64>,
) -> BTreeMap<String, f64> {
    base.iter()
        .filter_map(|(key, reference)| {
            let value = *candidate.get(key)?;
            let usable = |v: f64| v.is_finite() && v > 0.;
            (usable(value) && usable(*reference)).then(|| (key.clone(), value / reference))
        })
        .collect()
}

/// Median and interquartile range of each metric's ratio, for metrics every round measured.
fn summarize(pairs: &[BTreeMap<String, f64>]) -> BTreeMap<String, Paired> {
    let mut by_metric: BTreeMap<&String, Vec<f64>> = BTreeMap::new();
    for pair in pairs {
        for (key, ratio) in pair {
            by_metric.entry(key).or_default().push(*ratio);
        }
    }
    by_metric
        .into_iter()
        .filter(|(_, ratios)| ratios.len() == pairs.len())
        .map(|(key, mut ratios)| {
            ratios.sort_by(f64::total_cmp);
            let quantile = |q: f64| {
                let position = q * (ratios.len() - 1) as f64;
                let (low, high) = (position.floor() as usize, position.ceil() as usize);
                let value = ratios[low] + (ratios[high] - ratios[low]) * (position - low as f64);
                (value * 10_000.).round() / 10_000.
            };
            let paired = Paired {
                rounds: ratios.len(),
                median: quantile(0.5),
                p25: quantile(0.25),
                p75: quantile(0.75),
            };
            (key.clone(), paired)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(fps: f64, main_ms: f64) -> Value {
        json!({
            "frames": [{"phase": "spawn", "avg_fps": fps, "low_1pct_fps": fps / 2., "p50_ms": 1000. / fps,
                "p99_ms": 2000. / fps, "max_ms": 0.0}],
            "metrics": {
                "cpu_spans.spawn.main_frame.mean_ms": {"mean": main_ms},
                "gpu_received.spawn.gpu_frame.p99_ms": {"mean": main_ms},
            },
        })
    }

    #[test]
    fn rounds_alternate_which_build_runs_first() {
        assert_eq!(order(0), [Side::Base, Side::Candidate]);
        assert_eq!(order(1), [Side::Candidate, Side::Base]);
        assert_eq!(order(2), order(0));
    }

    #[test]
    fn ratios_pair_shared_positive_metrics() {
        let ratios = ratios(
            &metric_values(&report(4000., 0.25)).unwrap(),
            &metric_values(&report(5000., 0.2)).unwrap(),
        );
        assert_eq!(ratios["frames.spawn.avg_fps"], 1.25);
        assert_eq!(ratios["cpu_spans.spawn.main_frame.mean_ms"], 0.8);
        assert!(!ratios.contains_key("gpu_received.spawn.gpu_frame.p99_ms"));
        assert!(
            !ratios.contains_key("frames.spawn.max_ms"),
            "zero is not a usable reference"
        );
    }

    #[test]
    fn slower_counts_pairs_rather_than_interpolated_quantiles() {
        let pairs = |ratios: &[f64]| -> Vec<_> {
            ratios
                .iter()
                .map(|ratio| BTreeMap::from([("frames.spawn.avg_fps".to_owned(), *ratio)]))
                .collect()
        };
        assert_eq!(
            mostly_slower(&pairs(&[0.8, 0.9, 0.99, 1.1])),
            ["frames.spawn.avg_fps"]
        );
        assert!(mostly_slower(&pairs(&[0.8, 0.9, 1.01, 1.1])).is_empty());
    }

    #[test]
    fn metrics_missing_from_any_round_are_reported_not_summarized() {
        let pairs = [
            BTreeMap::from([("a".to_owned(), 0.5), ("b".to_owned(), 1.0)]),
            BTreeMap::from([("b".to_owned(), 1.0)]),
        ];
        let observed = BTreeSet::from(["a", "b", "removed"].map(str::to_owned));
        assert_eq!(unpaired(&observed, &pairs), ["a", "removed"]);
        assert_eq!(summarize(&pairs).keys().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn summary_reports_median_and_interquartile_range() {
        let pairs: Vec<_> = [0.9, 1.1, 1.0, 1.2, 0.8]
            .into_iter()
            .map(|ratio| BTreeMap::from([("frames.spawn.avg_fps".to_owned(), ratio)]))
            .collect();
        assert_eq!(
            summarize(&pairs)["frames.spawn.avg_fps"],
            Paired {
                rounds: 5,
                median: 1.0,
                p25: 0.9,
                p75: 1.1,
            }
        );
    }
}
