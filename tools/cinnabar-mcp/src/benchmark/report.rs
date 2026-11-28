//! Shared-shape summaries and conservative comparison of local diagnostic reports.

use super::{Plan, files, trace::Reduced};
use diagnostics::benchmark::Summary;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

const FORMAT: &str = "horizon-benchmark-report";
const MANIFEST_FORMAT: &str = "cinnabar-benchmark-manifest";
const VERSION: u32 = 1;

/// Writes one summary and a linked companion with explicit measurement gaps.
#[expect(
    clippy::too_many_arguments,
    reason = "report construction consumes the independently collected run artifacts"
)]
pub(super) fn write(
    output: &Path,
    plan: &Plan,
    initial: &Value,
    effective: &[Value],
    mut hardware: Value,
    memory: Value,
    mut reduced: Reduced,
    hashes: Value,
) -> Result<(), String> {
    hardware["gpu"] = initial["gpu"].clone();
    for (name, key) in [
        ("startup.title_ms", "title_ms"),
        ("startup.world_ms", "world_ms"),
    ] {
        let value = initial[key]
            .as_f64()
            .ok_or("completed startup endpoint missing")?;
        reduced.metrics.insert(name.into(), Summary::single(value)?);
    }
    if let Some(peak) = memory["sampled_family_peak_bytes"].as_u64() {
        reduced.metrics.insert(
            "memory.rss_peak_mb".into(),
            Summary::single(peak as f64 / 1048576.)?,
        );
    }
    let routes = serde_json::to_value(&plan.routes).map_err(|e| e.to_string())?;
    let mut settings = comparable_settings(initial);
    if effective
        .iter()
        .any(|state| comparable_settings(state) != settings)
    {
        return Err("effective settings changed during the benchmark".into());
    }
    settings["camera"] = effective.first().ok_or("no completed route")?["camera"].clone();
    let conditions = json!({
        "scenario": format!("frozen-terrain:{}", hashes["scene"].as_str().ok_or("missing scene hash")?),
        "render_distance": plan.render_distance, "window": plan.window, "vsync": false,
        "offscreen": true, "repeats": 1, "warmup_s": plan.warmup_s, "power": plan.power,
        "backend": initial["gpu"]["backend"],
    });
    let comparison_key = json!({
        "machine_id": plan.machine_id, "hardware": hardware, "settings": settings,
        "scene": hashes["scene"], "server": hashes["server"], "cache": hashes["cache"],
        "routes": routes, "conditions": conditions,
        "phase_cameras": effective.iter().map(|state| state["camera"].clone()).collect::<Vec<_>>(),
        "resident_wait": {"shape": "inscribed_square", "radius_columns": super::plan::resident_square_radius(plan.render_distance)},
        "build_profile": initial["build_profile"], "game_version": initial["game_version"], "protocol_version": initial["protocol_version"],
        "semantics": "resident-camera-real-frame-start-v1-stage-trace",
        "instrumentation": "developer-control-and-full-stage-trace", "server_placement": "same_machine",
    });
    let created = time::OffsetDateTime::now_utc();
    let created = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        created.year(),
        u8::from(created.month()),
        created.day(),
        created.hour(),
        created.minute(),
        created.second()
    );
    let report = json!({
        "format": FORMAT, "version": VERSION, "kind": "game", "created": created,
        "app": initial["app"], "hardware": hardware, "conditions": conditions,
        "metrics": reduced.metrics, "frames": reduced.frames,
        "local": ["manifest.json", "stages.json", "attempt.json", "inputs.json", "outcome.json"],
    });
    files::write(&output.join("report.json"), &report)?;
    let companion = json!({
        "format": MANIFEST_FORMAT, "version": VERSION, "status": "complete",
        "run_id": format!("{}-{}", std::process::id(), time::OffsetDateTime::now_utc().unix_timestamp_nanos()),
        "comparison_key": comparison_key, "hashes": hashes, "memory": memory,
        "build": {"app": initial["app"], "profile": initial["build_profile"], "game": initial["game_version"], "protocol": initial["protocol_version"]},
        "report_sha256": files::hash_file(&output.join("report.json"))?,
        "trace_sha256": files::hash_file(&output.join("stages.json"))?,
        "phases": reduced.phases, "sample_loss": 0, "qualification": "incomplete",
        "measurement_semantics": {
            "frames": "main frame-start intervals; not displayed FPS",
            "cpu": "native elapsed stage spans overlap; no common CPU aggregate",
            "gpu": "sampled execution durations grouped by receipt time; no originating frame or phase identity",
            "startup": "process entry to existing GPU-completed endpoints; excludes parent spawn-to-entry",
            "memory_mb": "bytes / 1048576; sampled family RSS, not native high-water",
            "motion": "camera only; player and stream anchor remain unchanged",
        },
        "unsupported": [
            "exact visible-cohort completion and route coverage", "authoritative streaming routes",
            "GPU source IDs and all-frame tail coverage", "OS display feedback", "shipping-artifact equivalence",
            "process-family CPU seconds", "native peak and process GPU memory", "per-role memory and sampler-gap ledger",
            "external power, thermal and contamination verification", "binary core/carrier and toolchain feature identities",
            "independent repeats and confidence intervals", "release and stability qualification",
        ],
    });
    files::write(&output.join("manifest.json"), &companion)
}

/// Selects only stable effective settings; startup measurements never affect comparability.
fn comparable_settings(state: &Value) -> Value {
    json!({"window": state["window"], "video": state["video"],
        "pacing": state["pacing"], "render_distance_blocks": state["render_distance_blocks"]})
}

/// Reads a report and its mandatory checksum-linked local companion.
fn load(path: &Path) -> Result<(Value, Value), String> {
    let report: Value = files::read(path)?;
    let companion: Value = files::read(&path.with_file_name("manifest.json"))?;
    if report["format"] != FORMAT
        || report["version"] != VERSION
        || report["kind"] != "game"
        || companion["format"] != MANIFEST_FORMAT
        || companion["version"] != VERSION
        || companion["status"] != "complete"
        || companion["sample_loss"] != 0
        || companion["report_sha256"] != files::hash_file(path)?
    {
        return Err("report format, completion, loss metadata or checksum is invalid".into());
    }
    Ok((report, companion))
}

/// Rejects unknown identity/conditions, unequal phase sets and any changed effective condition.
pub(super) fn compatible(a: &Value, am: &Value, b: &Value, bm: &Value) -> Result<(), String> {
    for manifest in [am, bm] {
        let key = &manifest["comparison_key"];
        for field in [
            "machine_id",
            "scene",
            "server",
            "cache",
            "semantics",
            "instrumentation",
            "server_placement",
        ] {
            if key[field].as_str().is_none_or(str::is_empty) {
                return Err(format!("unknown comparison {field}"));
            }
        }
        for pointer in [
            "/hardware/os_version",
            "/hardware/cpu/name",
            "/hardware/gpu/name",
            "/hardware/gpu/backend",
            "/hardware/gpu/driver",
            "/settings/camera/viewport",
            "/settings/camera/projection",
            "/settings/video/native_video_options",
        ] {
            if key
                .pointer(pointer)
                .is_none_or(|v| v.is_null() || v.as_str() == Some(""))
            {
                return Err(format!("unknown comparison condition {pointer}"));
            }
        }
        if key["routes"].as_array().is_none_or(Vec::is_empty) {
            return Err("missing routes".into());
        }
        for field in [
            "scenario",
            "render_distance",
            "window",
            "vsync",
            "offscreen",
            "repeats",
            "warmup_s",
            "power",
            "backend",
        ] {
            if key["conditions"].get(field).is_none_or(Value::is_null) {
                return Err(format!("unknown required condition {field}"));
            }
        }
        if key["conditions"].get("fps_cap").is_some_and(Value::is_null) {
            return Err("unknown FPS cap".into());
        }
    }
    if a["conditions"] != am["comparison_key"]["conditions"]
        || b["conditions"] != bm["comparison_key"]["conditions"]
        || a["hardware"] != am["comparison_key"]["hardware"]
        || b["hardware"] != bm["comparison_key"]["hardware"]
    {
        return Err("report conditions do not match their companion".into());
    }
    if a["conditions"] != b["conditions"]
        || a["hardware"] != b["hardware"]
        || am["comparison_key"] != bm["comparison_key"]
    {
        return Err("reports have different hardware, scene, route, cache, settings or measurement conditions".into());
    }
    let phases = |report: &Value| -> Result<BTreeMap<String, u64>, String> {
        let mut phases = BTreeMap::new();
        for phase in report["frames"].as_array().ok_or("missing frames")? {
            let name = phase["phase"].as_str().ok_or("missing phase name")?;
            let count = phase["frames"]
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or("missing frame count")?;
            if phases.insert(name.to_owned(), count).is_some() {
                return Err("duplicate phase".into());
            }
        }
        if phases.is_empty() {
            return Err("no measured phases".into());
        }
        Ok(phases)
    };
    let ap = phases(a)?;
    let bp = phases(b)?;
    let expected = am["comparison_key"]["routes"]
        .as_array()
        .ok_or("missing routes")?;
    if ap.keys().ne(bp.keys())
        || expected.len() != ap.len()
        || expected.iter().any(|route| {
            route["name"]
                .as_str()
                .is_none_or(|name| !ap.contains_key(name))
        })
    {
        return Err("reports do not contain every required phase".into());
    }
    Ok(())
}

/// Prints frame-interval ratios only after strict condition validation; no qualification is inferred.
#[expect(
    clippy::print_stdout,
    reason = "the explicit comparison CLI returns its JSON result on stdout"
)]
pub(super) fn compare(first: &Path, second: &Path) -> Result<(), String> {
    let (a, am) = load(first)?;
    let (b, bm) = load(second)?;
    compatible(&a, &am, &b, &bm)?;
    let mut ratios = BTreeMap::new();
    for phase in a["frames"].as_array().ok_or("missing frames")? {
        let other = b["frames"]
            .as_array()
            .ok_or("missing frames")?
            .iter()
            .find(|other| other["phase"] == phase["phase"])
            .ok_or("missing matching phase")?;
        let mut row = BTreeMap::new();
        for metric in [
            "avg_fps",
            "low_1pct_fps",
            "low_01pct_fps",
            "p50_ms",
            "p95_ms",
            "p99_ms",
            "p999_ms",
            "max_ms",
        ] {
            let value = phase[metric]
                .as_f64()
                .filter(|v| v.is_finite() && *v > 0.)
                .ok_or("invalid comparison metric")?;
            let reference = other[metric]
                .as_f64()
                .filter(|v| v.is_finite() && *v > 0.)
                .ok_or("invalid reference metric")?;
            row.insert(metric, value / reference);
        }
        ratios.insert(phase["phase"].as_str().ok_or("missing phase")?, row);
    }
    println!(
        "{}",
        json!({"comparison": "matched_diagnostic_conditions", "first_over_second": ratios,
        "qualification": "incomplete", "gpu_comparison": "unsupported_without_origin_ids"})
    );
    Ok(())
}
