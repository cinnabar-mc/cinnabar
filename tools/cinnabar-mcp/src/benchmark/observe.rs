//! Read-only host and process-family observations for diagnostic runs.

use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// Runs a numeric or hardware probe without collecting hostnames, environments or account data.
fn probe(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("{program} hardware/memory probe failed"));
    }
    String::from_utf8(output.stdout)
        .map(|v| v.trim().to_owned())
        .map_err(|e| e.to_string())
}

/// Reads OS, CPU and RAM identity; the client supplies the selected adapter and driver separately.
pub(super) fn hardware() -> Result<Value, String> {
    let os = std::env::consts::OS;
    let (cpu, ram_bytes) = match os {
        "macos" => (
            probe("sysctl", &["-n", "machdep.cpu.brand_string"])?,
            probe("sysctl", &["-n", "hw.memsize"])?
                .parse::<u64>()
                .map_err(|e| e.to_string())?,
        ),
        "linux" => {
            let cpu = std::fs::read_to_string("/proc/cpuinfo").map_err(|e| e.to_string())?;
            let name = cpu
                .lines()
                .find_map(|line| {
                    line.strip_prefix("model name")
                        .and_then(|v| v.split_once(':'))
                        .map(|(_, v)| v.trim().to_owned())
                })
                .ok_or("CPU identity unavailable")?;
            let memory = std::fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string())?;
            let kib = memory
                .lines()
                .find_map(|line| line.strip_prefix("MemTotal:"))
                .and_then(|v| v.split_whitespace().next())
                .ok_or("RAM identity unavailable")?
                .parse::<u64>()
                .map_err(|e| e.to_string())?;
            (name, kib * 1024)
        }
        _ => return Err("benchmark host probes currently require Linux or macOS".into()),
    };
    Ok(
        json!({"os": os, "os_version": probe("uname", &["-srv"])? , "arch": std::env::consts::ARCH,
        "cpu": {"name": cpu, "logical_cores": thread::available_parallelism().map_err(|e| e.to_string())?.get()}, "ram_mb": ram_bytes as f64 / 1048576.}),
    )
}

/// A bounded-lifetime sampler owned by the exact client process created by this runner.
pub(super) struct Sampler {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Value>>,
}

impl Sampler {
    /// Samples RSS for the root and its current descendants from immediately after process spawn.
    pub(super) fn start(pid: u32) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let mut peak = 0_u64;
            let mut samples = 0_u64;
            let mut failed = 0_u64;
            while !stopping.load(Ordering::Acquire) {
                match probe("ps", &["-axo", "pid=,ppid=,rss="])
                    .and_then(|text| family_rss(&text, pid))
                {
                    Ok(bytes) => {
                        peak = peak.max(bytes);
                        samples += 1;
                    }
                    Err(_) => failed += 1,
                }
                thread::sleep(Duration::from_millis(250));
            }
            json!({"sampled_family_peak_bytes": (samples > 0).then_some(peak), "samples": samples,
                "failed_samples": failed, "interval_ms": 250, "method": "sum_current_descendant_rss",
                "limitations": ["shared pages may be counted more than once", "short-lived or reparented helpers can be missed", "OS native high-water measurement unavailable"]})
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }

    /// Stops observation after acknowledged client shutdown and joins only this owned worker.
    pub(super) fn finish(mut self) -> Value {
        self.stop.store(true, Ordering::Release);
        self.worker
            .take()
            .and_then(|worker| worker.join().ok())
            .unwrap_or_else(|| json!({"error": "sampler failed"}))
    }
}

impl Drop for Sampler {
    /// Stops the observer even when launch, capture or shutdown fails.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Sums only the selected process tree, using numeric identities rather than process-name patterns.
pub(super) fn family_rss(text: &str, root: u32) -> Result<u64, String> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let values = line
            .split_whitespace()
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if values.len() != 3 {
            return Err("invalid process memory row".into());
        }
        rows.push((values[0], values[1], values[2]));
    }
    if !rows.iter().any(|row| row.0 == u64::from(root)) {
        return Err("owned client has exited".into());
    }
    let mut selected = BTreeSet::from([u64::from(root)]);
    loop {
        let before = selected.len();
        for &(pid, parent, _) in &rows {
            if selected.contains(&parent) {
                selected.insert(pid);
            }
        }
        if before == selected.len() {
            break;
        }
    }
    Ok(rows
        .iter()
        .filter(|row| selected.contains(&row.0))
        .map(|row| row.2 * 1024)
        .sum())
}
