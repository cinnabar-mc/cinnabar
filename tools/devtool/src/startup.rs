//! Release launch acceptance with presentation-based samples and separate cache modes.

use crate::DevtoolError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};

const WARM_CACHE: &str = "warm";
const COLD_CACHE: &str = "cold";
/// Each supported driver override owns its isolated directory name.
const DRIVER_CACHE_ENVIRONMENT: [(&str, &str); 2] = [
    ("MESA_SHADER_CACHE_DIR", "mesa"),
    ("__GL_SHADER_DISK_CACHE_PATH", "gl"),
];

mod process;
mod timeline;
use process::OwnedClient;

#[derive(Debug)]
struct Options {
    client: PathBuf,
    runs: usize,
    modes: Vec<&'static str>,
    budget_ms: f64,
    cold_budget_ms: f64,
    out: Option<PathBuf>,
    baseline: Option<PathBuf>,
    cold_reset: Option<PathBuf>,
    carriers: Vec<&'static str>,
    carrier_dir: Option<PathBuf>,
    carrier_reset: Option<PathBuf>,
    profile: Option<PathBuf>,
    args: Vec<String>,
    offscreen: bool,
}

#[derive(Serialize, Deserialize)]
struct Summary {
    #[serde(default)]
    offscreen: bool,
    modes: Vec<Mode>,
}

#[derive(Serialize, Deserialize)]
struct Mode {
    cache: String,
    #[serde(default = "warm_carriers")]
    carriers: String,
    budget_ms: f64,
    menu: Distribution,
    stages: BTreeMap<String, Distribution>,
    #[serde(default)]
    timeline: Vec<timeline::Segment>,
    #[serde(default)]
    pipelines: BTreeMap<String, timeline::PipelineLatency>,
    #[serde(default)]
    work: BTreeMap<String, u64>,
    samples: Vec<Value>,
}

/// Results saved before carrier modes existed measured warm carriers.
fn warm_carriers() -> String {
    WARM_CACHE.into()
}

#[derive(Serialize, Deserialize)]
struct Distribution {
    median_ms: f64,
    p99_ms: f64,
}

/// Parses an explicit executable and forwards arguments after `--` unchanged.
fn parse(args: &[String]) -> Result<Options, DevtoolError> {
    let mut options = Options {
        client: PathBuf::from("target/release/cinnabar"),
        runs: 30,
        modes: vec![WARM_CACHE],
        budget_ms: 1000.0,
        cold_budget_ms: 4000.0,
        out: None,
        baseline: None,
        cold_reset: None,
        carriers: vec![WARM_CACHE],
        carrier_dir: None,
        carrier_reset: None,
        profile: None,
        args: Vec::new(),
        offscreen: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            options.args = args.cloned().collect();
            break;
        }
        let value = args
            .next()
            .ok_or_else(|| error(format!("{arg} requires a value")))?;
        match arg.as_str() {
            "--client" => options.client = value.into(),
            "--runs" => options.runs = value.parse().map_err(|_| error("invalid run count"))?,
            "--cache" => {
                options.modes = match value.as_str() {
                    WARM_CACHE => vec![WARM_CACHE],
                    COLD_CACHE => vec![COLD_CACHE],
                    "both" => vec![WARM_CACHE, COLD_CACHE],
                    _ => return Err(error("--cache must be warm, cold or both")),
                }
            }
            "--carriers" => {
                options.carriers = match value.as_str() {
                    WARM_CACHE => vec![WARM_CACHE],
                    COLD_CACHE => vec![COLD_CACHE],
                    "both" => vec![WARM_CACHE, COLD_CACHE],
                    _ => return Err(error("--carriers must be warm, cold or both")),
                }
            }
            "--carrier-dir" => options.carrier_dir = Some(value.into()),
            "--carrier-reset" => options.carrier_reset = Some(value.into()),
            // The client rejects a relative user root.
            "--profile" => options.profile = Some(std::path::absolute(value).map_err(error)?),
            "--budget-ms" => options.budget_ms = positive(value)?,
            "--cold-budget-ms" => options.cold_budget_ms = positive(value)?,
            "--endpoint" => {
                options.offscreen = match value.as_str() {
                    "native" => false,
                    "offscreen" => true,
                    _ => return Err(error("--endpoint must be native or offscreen")),
                };
            }
            "--out" => options.out = Some(value.into()),
            "--baseline" => options.baseline = Some(value.into()),
            "--cold-reset" => options.cold_reset = Some(value.into()),
            _ => return Err(error(format!("unknown startup option {arg}"))),
        }
    }
    if options.runs == 0 {
        return Err(error("--runs must be positive"));
    }
    if options
        .client
        .parent()
        .is_some_and(|parent| parent.file_name().is_some_and(|name| name == "debug"))
    {
        return Err(error("startup acceptance requires a release client"));
    }
    if cfg!(any(target_os = "macos", target_os = "windows"))
        && options.modes.contains(&COLD_CACHE)
        && options.cold_reset.is_none()
    {
        return Err(error(
            "cold cache runs require --cold-reset on this platform",
        ));
    }
    if options.profile.is_some()
        && options.modes.contains(&COLD_CACHE)
        && options.cold_reset.is_none()
    {
        return Err(error(
            "cold cache runs with --profile require --cold-reset to clear that profile's caches",
        ));
    }
    if options.carriers.contains(&COLD_CACHE)
        && options.carrier_reset.is_none()
        && (!cfg!(target_os = "linux") || options.carrier_dir.is_none())
    {
        return Err(error(
            "cold carrier runs require --carrier-dir on Linux or --carrier-reset elsewhere",
        ));
    }
    Ok(options)
}

/// Rejects NaN, infinity and nonpositive budgets before any client is launched.
fn positive(value: &str) -> Result<f64, DevtoolError> {
    let value: f64 = value.parse().map_err(|_| error("invalid budget"))?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(error("budget must be finite and positive"))
    }
}

/// Gives startup command failures their own searchable prefix.
fn error(message: impl std::fmt::Display) -> DevtoolError {
    DevtoolError::Usage(format!("startup: {message}"))
}

/// Launches one owned process and waits for its structured report and orderly exit.
fn launch(options: &Options, cache: &Path) -> Result<Value, DevtoolError> {
    let mut command = Command::new(&options.client);
    // An installed client keeps its prepared carriers in its profile, so it names one to reuse.
    let profile = options
        .profile
        .clone()
        .unwrap_or_else(|| cache.join("profile"));
    std::fs::create_dir_all(&profile).map_err(error)?;
    command
        .args(&options.args)
        .env(render_api::STARTUP_BENCHMARK_ENV, "1")
        .env(render_api::USER_ROOT_ENV, &profile)
        .env("NO_COLOR", "1");
    for (variable, directory) in DRIVER_CACHE_ENVIRONMENT {
        let directory = cache.join(directory);
        std::fs::create_dir_all(&directory).map_err(error)?;
        command.env(variable, directory);
    }
    let child = OwnedClient::spawn(command).map_err(|source| DevtoolError::Spawn {
        command: options.client.display().to_string(),
        source,
    })?;
    collect(
        child,
        options.offscreen,
        Instant::now() + Duration::from_secs(30),
    )
}

/// Drains child output and reaps the client within the same watchdog boundary.
fn collect(
    mut child: OwnedClient,
    offscreen: bool,
    deadline: Instant,
) -> Result<Value, DevtoolError> {
    let (send, lines) = mpsc::sync_channel(256);
    let mut readers = Vec::new();
    for pipe in [
        Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
        Box::new(child.stderr.take().unwrap()),
    ] {
        let send = send.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        }));
    }
    drop(send);
    let mut report = None;
    let mut tail = std::collections::VecDeque::new();
    let mut status = None;
    let mut drained = false;
    let result = loop {
        if status.is_none() {
            status = child.try_wait().map_err(error)?;
        }
        if let Some(status) = status.filter(|_| drained) {
            break if status.success() {
                report.ok_or_else(|| {
                    error("client exited without a menu report; build with acceptance enabled")
                })
            } else {
                Err(error(format!(
                    "client exited {status}: {}",
                    tail.into_iter().collect::<Vec<_>>().join("\n")
                )))
            };
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break Err(error(
                "client did not present a menu and exit within the launch watchdog",
            ));
        }
        match lines.recv_timeout(left.min(Duration::from_millis(50))) {
            Ok(line) => {
                observe(&line, &mut report, &mut tail)?;
                if let Some(report) = &report {
                    validate_endpoint(report, offscreen)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                drained = true;
                // No output remains; wait on the process without spinning on the closed channel.
                std::thread::park_timeout(left.min(Duration::from_millis(50)));
            }
        }
    };
    drop(child);
    drop(lines);
    for reader in readers {
        if reader.is_finished() {
            let _ = reader.join();
        }
    }
    result
}

/// Retains a bounded failure tail and accepts only a finite presentation measurement.
fn observe(
    line: &str,
    report: &mut Option<Value>,
    tail: &mut std::collections::VecDeque<String>,
) -> Result<(), DevtoolError> {
    if let Some((_, json)) = line.split_once(render_api::STARTUP_TIMING_MARKER) {
        let parsed: Value = serde_json::Deserializer::from_str(json.trim())
            .into_iter()
            .next()
            .ok_or_else(|| error("empty startup report"))?
            .map_err(|failure| error(format!("invalid startup report: {failure}")))?;
        measurement(&parsed, "menu_ms")?;
        if parsed["debug_assertions"].as_bool() != Some(false) {
            return Err(error("startup acceptance requires a release client report"));
        }
        if report.is_some() {
            return Err(error("client emitted more than one startup report"));
        }
        if !parsed["stages_ms"].is_object() {
            return Err(error("startup report has no stage table"));
        }
        *report = Some(parsed);
    }
    if tail.len() == 20 {
        tail.pop_front();
    }
    tail.push_back(line.into());
    Ok(())
}

/// Keeps CPU-only image comparisons separate from native presentation acceptance.
fn validate_endpoint(report: &Value, offscreen: bool) -> Result<(), DevtoolError> {
    if report["offscreen"].as_bool() != Some(offscreen) {
        return Err(error(
            "client report endpoint differs from --endpoint; hidden images require --endpoint offscreen",
        ));
    }
    Ok(())
}

/// Reads a duration without accepting missing, negative or nonnumeric measurements.
fn measurement(sample: &Value, key: &str) -> Result<f64, DevtoolError> {
    sample[key]
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| error(format!("missing or invalid {key}")))
}

/// Uses the nearest-rank p99 and the middle pair for an even-sized median.
fn distribution(mut samples: Vec<f64>) -> Distribution {
    samples.sort_by(f64::total_cmp);
    let middle = samples.len() / 2;
    let median_ms = if samples.len().is_multiple_of(2) {
        (samples[middle - 1] + samples[middle]) / 2.0
    } else {
        samples[middle]
    };
    let index = (samples.len() * 99).div_ceil(100) - 1;
    Distribution {
        median_ms,
        p99_ms: samples[index],
    }
}

/// Summarizes all stages the client emitted, without a duplicated stage list.
fn summarize(
    cache: &str,
    carriers: &str,
    budget_ms: f64,
    samples: Vec<Value>,
) -> Result<Mode, DevtoolError> {
    if samples.is_empty() {
        return Err(error("no launch samples"));
    }
    let menu = distribution(
        samples
            .iter()
            .map(|sample| measurement(sample, "menu_ms"))
            .collect::<Result<_, _>>()?,
    );
    let mut stages: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for sample in &samples {
        let mut times = sample["stages_ms"]
            .as_object()
            .ok_or_else(|| error("missing stage table"))?
            .clone();
        if let Some(frames) = sample["render_frames"].as_array() {
            for (index, frame) in frames.iter().enumerate() {
                if let Some(frame) = frame.as_object() {
                    for (name, duration) in frame.iter().filter(|(name, _)| name.ends_with("_ms")) {
                        times.insert(format!("render_{}_{name}", index + 1), duration.clone());
                    }
                }
            }
        }
        if let Some(updates) = sample["main_updates_ms"].as_array() {
            for (index, duration) in updates.iter().enumerate() {
                times.insert(format!("main_update_{}", index + 1), duration.clone());
            }
        }
        for (name, value) in times {
            let value = value
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or_else(|| error(format!("invalid stage {name}")))?;
            stages.entry(name).or_default().push(value);
        }
    }
    Ok(Mode {
        cache: cache.into(),
        carriers: carriers.into(),
        budget_ms,
        menu,
        stages: stages
            .into_iter()
            .map(|(name, values)| (name, distribution(values)))
            .collect(),
        timeline: timeline::summarize(&samples),
        pipelines: timeline::pipelines(&samples),
        work: timeline::work(&samples),
        samples,
    })
}

/// Reports every stage's movement against the same cache mode in a saved acceptance result.
fn growth(mode: &Mode, baseline: Option<&Mode>) -> String {
    mode.stages
        .iter()
        .filter_map(|(name, current)| {
            let previous = baseline?
                .stages
                .get(name)
                .map_or(0.0, |previous| previous.p99_ms);
            let delta = current.p99_ms - previous;
            (delta > 0.0).then(|| format!("{name} +{delta:.2} ms"))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Prints one mode's critical-path partition, pipeline latency, work and stage tables.
fn report(mode: &Mode, baseline: Option<&Mode>) {
    println!(
        "{} cache, {} carriers: n={} menu median={:.2} ms p99={:.2} ms budget={:.2} ms",
        mode.cache,
        mode.carriers,
        mode.samples.len(),
        mode.menu.median_ms,
        mode.menu.p99_ms,
        mode.budget_ms
    );
    println!("segment\tlaunches\tstarts at ms\tmedian ms\tp99 ms");
    for segment in &mode.timeline {
        println!(
            "{} -> {}\t{}\t{:.2}\t{:.2}\t{:.2}",
            segment.from,
            segment.to,
            segment.samples,
            segment.start.median_ms,
            segment.duration.median_ms,
            segment.duration.p99_ms
        );
    }
    println!("pipeline\tready at ms\tlatency median ms\tp99 ms\tunready at menu");
    for (name, pipeline) in &mode.pipelines {
        let (ready, latency) = (pipeline.ready.as_ref(), pipeline.latency.as_ref());
        println!(
            "{name}\t{}\t{}\t{}\t{}",
            ready.map_or("-".into(), |ready| format!("{:.2}", ready.median_ms)),
            latency.map_or("-".into(), |latency| format!("{:.2}", latency.median_ms)),
            latency.map_or("-".into(), |latency| format!("{:.2}", latency.p99_ms)),
            pipeline.unready
        );
    }
    println!("work\tmaximum");
    for (name, value) in &mode.work {
        println!("{name}\t{value}");
    }
    if let Some(baseline) = baseline {
        for grew in timeline::work_growth(&mode.work, &baseline.work) {
            println!("work grew: {grew}");
        }
    }
    println!("stage\tmedian ms\tp99 ms");
    for (name, sample) in &mode.stages {
        println!("{name}\t{:.2}\t{:.2}", sample.median_ms, sample.p99_ms);
    }
}

/// Runs a reset program and fails the benchmark if it does not succeed.
fn reset_with(program: &Path, purpose: &str) -> Result<(), DevtoolError> {
    let status = Command::new(program)
        .status()
        .map_err(|source| DevtoolError::Spawn {
            command: program.display().to_string(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(error(format!("{purpose} exited {status}")))
    }
}

/// Drops the carriers from the OS page cache so the next launch reads them from storage.
fn evict_carriers(options: &Options) -> Result<(), DevtoolError> {
    if let Some(reset) = &options.carrier_reset {
        return reset_with(reset, "cold carrier reset");
    }
    #[cfg(target_os = "linux")]
    if let Some(directory) = &options.carrier_dir {
        return evict_directory(directory);
    }
    Err(error("cold carriers need --carrier-dir or --carrier-reset"))
}

/// Advises the kernel to discard every clean cached page of the files under `directory`.
#[cfg(target_os = "linux")]
fn evict_directory(directory: &Path) -> Result<(), DevtoolError> {
    use std::os::fd::AsRawFd;
    for entry in std::fs::read_dir(directory).map_err(error)? {
        let path = entry.map_err(error)?.path();
        if path.is_dir() {
            evict_directory(&path)?;
            continue;
        }
        let file = std::fs::File::open(&path).map_err(error)?;
        // SAFETY: the descriptor stays open for the call; advice cannot change file contents.
        let result =
            unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
        if result != 0 {
            return Err(error(format!(
                "evict {}: {}",
                path.display(),
                std::io::Error::from_raw_os_error(result)
            )));
        }
    }
    Ok(())
}

/// Runs warm and cold driver-cache launches and enforces their p99 budgets.
pub(crate) fn run(args: &[String]) -> Result<(), DevtoolError> {
    let options = parse(args)?;
    let caches = tempfile::tempdir().map_err(error)?;
    let baseline: Option<Summary> = options
        .baseline
        .as_ref()
        .map(|path| {
            let bytes = std::fs::read(path).map_err(error)?;
            serde_json::from_slice(&bytes).map_err(error)
        })
        .transpose()?;
    if baseline
        .as_ref()
        .is_some_and(|baseline| baseline.offscreen != options.offscreen)
    {
        return Err(error(
            "baseline endpoint differs from the requested endpoint",
        ));
    }
    let mut summary = Summary {
        offscreen: options.offscreen,
        modes: Vec::new(),
    };
    for &cache in &options.modes {
        let warm = caches.path().join(WARM_CACHE);
        if cache == WARM_CACHE {
            launch(&options, &warm)?;
        }
        for &carriers in &options.carriers {
            if cache == COLD_CACHE && carriers == WARM_CACHE {
                // Read the carriers once, unmeasured, in a directory no sample uses: cold runs
                // otherwise start with whatever an earlier cold-carrier mode evicted.
                launch(
                    &options,
                    &caches.path().join(format!("carrier-warmup-{cache}")),
                )?;
            }
            let mut samples = Vec::with_capacity(options.runs);
            for index in 0..options.runs {
                let directory = if cache == WARM_CACHE {
                    warm.clone()
                } else {
                    caches.path().join(format!("cold-{carriers}-{index}"))
                };
                if cache == COLD_CACHE
                    && let Some(reset) = &options.cold_reset
                {
                    reset_with(reset, "cold cache reset")?;
                }
                if carriers == COLD_CACHE {
                    evict_carriers(&options)?;
                }
                samples.push(launch(&options, &directory)?);
            }
            let budget = if cache == WARM_CACHE {
                options.budget_ms
            } else {
                options.cold_budget_ms
            };
            summary
                .modes
                .push(summarize(cache, carriers, budget, samples)?);
        }
    }
    if let Some(path) = &options.out {
        std::fs::write(path, serde_json::to_vec_pretty(&summary).map_err(error)?).map_err(error)?;
    }
    let mut failed = false;
    println!(
        "endpoint: {}",
        if options.offscreen {
            "offscreen CPU comparison (no native presentation acceptance)"
        } else {
            "native presentation"
        }
    );
    for mode in &summary.modes {
        let prior = baseline.as_ref().and_then(|summary| {
            summary
                .modes
                .iter()
                .find(|prior| prior.cache == mode.cache && prior.carriers == mode.carriers)
        });
        report(mode, prior);
        if mode.menu.p99_ms > mode.budget_ms {
            failed = true;
            let grew = growth(mode, prior);
            eprintln!(
                "{} cache, {} carriers: startup exceeds budget; stage growth: {}",
                mode.cache,
                mode.carriers,
                if grew.is_empty() {
                    "no comparable baseline; use --baseline to attribute growth"
                } else {
                    &grew
                }
            );
        }
    }
    if failed {
        Err(error("launch-to-menu p99 exceeds budget"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offscreen_samples_cannot_satisfy_native_presentation_acceptance() {
        for offscreen in [false, true] {
            let report = serde_json::json!({"offscreen": offscreen});
            assert!(validate_endpoint(&report, offscreen).is_ok());
            assert!(validate_endpoint(&report, !offscreen).is_err());
        }
        assert!(validate_endpoint(&serde_json::json!({}), false).is_err());
    }

    #[test]
    fn quantiles_and_stage_growth_use_samples_rather_than_wall_clock() {
        let values = (1..=100).map(|n| n as f64).collect();
        let result = distribution(values);
        assert_eq!(result.median_ms, 50.5);
        assert_eq!(result.p99_ms, 99.0);
        let sample =
            |menu, stage| serde_json::json!({"menu_ms": menu, "stages_ms": {"device": stage}});
        let before = summarize(WARM_CACHE, WARM_CACHE, 10.0, vec![sample(5.0, 2.0)]).unwrap();
        let after = summarize(WARM_CACHE, WARM_CACHE, 10.0, vec![sample(11.0, 4.0)]).unwrap();
        assert!(after.menu.p99_ms > after.budget_ms);
        assert_eq!(growth(&after, Some(&before)), "device +2.00 ms");
    }

    #[test]
    fn invalid_budgets_and_reports_cannot_pass_acceptance() {
        for value in ["NaN", "inf", "0", "-1"] {
            assert!(positive(value).is_err());
        }
        let cold = ["--cache".to_owned(), COLD_CACHE.to_owned()];
        assert_eq!(
            parse(&cold).is_err(),
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
        let mut reset = cold.to_vec();
        reset.extend(["--cold-reset".to_owned(), "reset-fixture".to_owned()]);
        assert!(parse(&reset).is_ok());
        let carriers = ["--carriers".to_owned(), COLD_CACHE.to_owned()];
        assert!(parse(&carriers).is_err());
        let mut carrier_reset = carriers.to_vec();
        carrier_reset.extend(["--carrier-reset".to_owned(), "reset-fixture".to_owned()]);
        assert!(parse(&carrier_reset).is_ok());
        let profiled = [
            "--profile".to_owned(),
            "installed".to_owned(),
            cold[0].clone(),
            cold[1].clone(),
        ];
        assert!(parse(&profiled).is_err());
        assert!(
            parse(&["--profile".to_owned(), "installed".to_owned()])
                .unwrap()
                .profile
                .unwrap()
                .is_absolute()
        );
        let mut profiled_reset = profiled.to_vec();
        profiled_reset.extend(["--cold-reset".to_owned(), "reset-fixture".to_owned()]);
        assert!(parse(&profiled_reset).is_ok());
        let mut carrier_dir = carriers.to_vec();
        carrier_dir.extend(["--carrier-dir".to_owned(), "assets".to_owned()]);
        assert_eq!(parse(&carrier_dir).is_ok(), cfg!(target_os = "linux"));
        let mut report = None;
        let mut tail = Default::default();
        let line = format!(
            "{} {{\"stages_ms\":{{}}}}",
            render_api::STARTUP_TIMING_MARKER
        );
        assert!(observe(&line, &mut report, &mut tail).is_err());
        assert!(
            summarize(
                WARM_CACHE,
                WARM_CACHE,
                1.0,
                vec![serde_json::json!({"menu_ms": -1})]
            )
            .is_err()
        );
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn inherited_output_pipes_cannot_extend_an_expired_watchdog() {
        let directory = tempfile::tempdir().unwrap();
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "mkfifo \"$1\"; (read line < \"$1\") & echo ready; exit 0",
                "fixture",
            ])
            .arg(directory.path().join("wait"));
        let mut child = OwnedClient::spawn(command).unwrap();
        let mut pipe = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        pipe.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "ready");
        child.stdout = Some(pipe.into_inner());
        child.wait().unwrap();
        let result = collect(child, false, Instant::now());
        assert!(result.unwrap_err().to_string().contains("watchdog"));
    }

    #[test]
    fn launch_requires_a_presented_report_and_successful_process_exit() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("release-client-fixture");
        let options = Options {
            client: executable.clone(),
            ..parse(&[]).unwrap()
        };
        let report = serde_json::json!({"debug_assertions": false, "offscreen": false, "menu_ms": 7, "stages_ms": {"device": 2}});
        for (line, code, succeeds) in [
            (
                format!("{} {report}", render_api::STARTUP_TIMING_MARKER),
                0,
                true,
            ),
            (String::new(), 0, false),
            (
                format!("{} {report}", render_api::STARTUP_TIMING_MARKER),
                1,
                false,
            ),
        ] {
            std::fs::write(
                &executable,
                format!("#!/bin/sh\nprintf '%s\\n' '{line}' >&2\nexit {code}\n"),
            )
            .unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let result = launch(&options, directory.path());
            for (_, name) in DRIVER_CACHE_ENVIRONMENT {
                assert!(directory.path().join(name).is_dir());
            }
            assert!(directory.path().join("profile").is_dir());
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
            if succeeds {
                assert_eq!(result.unwrap()["menu_ms"], 7);
            }
        }
    }

    #[test]
    fn cold_cache_runs_with_warm_carriers_warm_them_outside_the_measured_caches() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("release-client-fixture");
        let log = directory.path().join("launches.log");
        let report = serde_json::json!({"debug_assertions": false, "offscreen": false, "menu_ms": 7, "stages_ms": {}});
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$MESA_SHADER_CACHE_DIR\" >> '{}'\nprintf '%s\\n' '{} {report}' >&2\n",
                log.display(),
                render_api::STARTUP_TIMING_MARKER
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let out = directory.path().join("summary.json");
        let args: Vec<String> = [
            "--client",
            executable.to_str().unwrap(),
            "--cache",
            COLD_CACHE,
            "--runs",
            "2",
            "--cold-reset",
            "/usr/bin/true",
            "--cold-budget-ms",
            "1000000",
            "--out",
            out.to_str().unwrap(),
        ]
        .map(String::from)
        .into();
        run(&args).unwrap();
        let launches: Vec<String> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(launches.len(), 3, "{launches:?}");
        assert!(launches[0].contains("carrier-warmup"), "{launches:?}");
        assert!(
            launches[1..]
                .iter()
                .all(|dir| !dir.contains("carrier-warmup"))
        );
        assert_ne!(launches[1], launches[2]);
        let summary: Summary = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
        assert_eq!(summary.modes[0].samples.len(), 2);
    }

    #[test]
    fn a_named_profile_replaces_the_isolated_one() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("release-client-fixture");
        let installed = directory.path().join("installed");
        let report = serde_json::json!({"debug_assertions": false, "offscreen": false, "menu_ms": 7, "stages_ms": {}});
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\ntouch \"${}/seen\"\nprintf '%s\\n' '{} {report}' >&2\n",
                render_api::USER_ROOT_ENV,
                render_api::STARTUP_TIMING_MARKER
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let options = Options {
            client: executable,
            profile: Some(installed.clone()),
            ..parse(&[]).unwrap()
        };
        launch(&options, &directory.path().join("cache")).unwrap();
        assert!(installed.join("seen").is_file());
        assert!(!directory.path().join("cache/profile").exists());
    }
}

/// Eviction succeeds for nested carrier files without changing their contents.
#[cfg(all(test, target_os = "linux"))]
#[test]
fn carrier_eviction_keeps_nested_files_intact() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("compiled")).unwrap();
    let carrier = directory.path().join("compiled/vanilla.mcbeui");
    std::fs::write(&carrier, b"carrier").unwrap();
    evict_directory(directory.path()).unwrap();
    assert_eq!(std::fs::read(carrier).unwrap(), b"carrier");
}
